use chrono::{DateTime, Utc};
use gitdiverge_lib::BranchAnalytics;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Maximum cache size in bytes (2 GB).
const DEFAULT_MAX_CACHE_BYTES: usize = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    repo_guid: String,
    branches: Vec<String>,
}

impl CacheKey {
    fn new(repo_guid: String, branches: &[String]) -> Self {
        let mut branches = branches.to_vec();
        branches.sort_unstable();
        Self {
            repo_guid,
            branches,
        }
    }
}

/// Cached divergence result for a single repository.
#[derive(Clone, Debug)]
pub struct CachedDivergence {
    /// Full branch analytics (includes every commit).
    pub analytics: BranchAnalytics,
    /// Timestamp when the result was computed.
    pub computed_at: DateTime<Utc>,
}

#[derive(Debug)]
struct CacheEntry {
    divergence: CachedDivergence,
    /// Approximate memory footprint of this entry in bytes.
    size: usize,
}

#[derive(Debug)]
struct CacheState {
    map: HashMap<CacheKey, CacheEntry>,
    total_size: usize,
    max_bytes: usize,
}

/// In-memory cache for computed divergence results.
///
/// The cache is safe to share across threads and tasks.  When the estimated
/// memory consumption of the stored entries exceeds the configured limit
/// (default 2 GB) the oldest entries are evicted until the limit is met.
#[derive(Clone, Debug)]
pub struct DivergenceCache {
    inner: Arc<RwLock<CacheState>>,
}

impl DivergenceCache {
    /// Create a new empty cache with the default 2 GB limit.
    pub fn new() -> Self {
        Self::with_max_bytes(DEFAULT_MAX_CACHE_BYTES)
    }

    fn with_max_bytes(max_bytes: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(CacheState {
                map: HashMap::new(),
                total_size: 0,
                max_bytes,
            })),
        }
    }

    /// Retrieve a cached result by repository GUID and branch list.
    pub fn get_with_branches(
        &self,
        repo_guid: &str,
        branches: &[String],
    ) -> Option<CachedDivergence> {
        let key = CacheKey::new(repo_guid.to_string(), branches);
        let state = self.inner.read().unwrap();
        state.map.get(&key).map(|e| e.divergence.clone())
    }

    /// Store a full `BranchAnalytics` result for the given GUID and branch list.
    ///
    /// If inserting the entry causes the cache to exceed its memory limit,
    /// oldest entries are evicted until the limit is respected.
    pub fn insert_with_branches(
        &self,
        repo_guid: String,
        branches: &[String],
        analytics: BranchAnalytics,
    ) {
        let key = CacheKey::new(repo_guid, branches);
        let size = estimated_size(&analytics);
        let entry = CacheEntry {
            divergence: CachedDivergence {
                analytics,
                computed_at: Utc::now(),
            },
            size,
        };

        let mut state = self.inner.write().unwrap();
        if let Some(old) = state.map.insert(key, entry) {
            state.total_size = state.total_size.saturating_sub(old.size);
        }
        state.total_size = state.total_size.saturating_add(size);
        self.evict_oldest(&mut state);
    }

    /// Remove all cached entries for a repository (e.g. after clone/fetch).
    pub fn invalidate_repo(&self, repo_guid: &str) {
        let mut state = self.inner.write().unwrap();
        let keys_to_remove: Vec<CacheKey> = state
            .map
            .keys()
            .filter(|k| k.repo_guid == repo_guid)
            .cloned()
            .collect();
        for key in keys_to_remove {
            if let Some(entry) = state.map.remove(&key) {
                state.total_size = state.total_size.saturating_sub(entry.size);
            }
        }
    }

    /// Clear the entire cache.
    pub fn invalidate_all(&self) {
        let mut state = self.inner.write().unwrap();
        state.map.clear();
        state.total_size = 0;
    }

    /// Returns the approximate total size of all cached entries in bytes.
    #[cfg(test)]
    fn total_size(&self) -> usize {
        self.inner.read().unwrap().total_size
    }

    /// Returns the number of entries currently in the cache.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.read().unwrap().map.len()
    }

    fn evict_oldest(&self, state: &mut CacheState) {
        if state.total_size <= state.max_bytes {
            return;
        }

        // Collect entries ordered by computation time (oldest first).
        let mut ordered: Vec<(CacheKey, DateTime<Utc>)> = state
            .map
            .iter()
            .map(|(k, v)| (k.clone(), v.divergence.computed_at))
            .collect();
        ordered.sort_by_key(|(_, t)| *t);

        let mut evicted = 0usize;
        for (key, _) in ordered {
            if state.total_size <= state.max_bytes {
                break;
            }
            if let Some(entry) = state.map.remove(&key) {
                state.total_size = state.total_size.saturating_sub(entry.size);
                evicted += 1;
                tracing::debug!(
                    repo_guid = %key.repo_guid,
                    size = entry.size,
                    "evicted divergence cache entry"
                );
            }
        }

        if evicted > 0 {
            tracing::info!(
                evicted,
                remaining_entries = state.map.len(),
                total_size = state.total_size,
                max_bytes = state.max_bytes,
                "divergence cache eviction completed"
            );
        }
    }
}

impl Default for DivergenceCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Produce a rough estimate of the heap memory consumed by `analytics`.
fn estimated_size(analytics: &BranchAnalytics) -> usize {
    let mut size = std::mem::size_of::<BranchAnalytics>();

    size += analytics.repo_path.len();
    size += analytics
        .branches
        .iter()
        .map(|s| std::mem::size_of::<String>() + s.len())
        .sum::<usize>();

    size += analytics
        .comparisons
        .iter()
        .map(|c| {
            let mut s = std::mem::size_of::<BranchComparison>();
            s += c.source_branch.len() + c.target_branch.len();
            s += c
                .missing_commits
                .iter()
                .map(|commit| {
                    let mut cs = std::mem::size_of::<Commit>() + std::mem::size_of::<Author>();
                    cs += commit.hash.len();
                    cs += commit.author.name.len() + commit.author.email.len();
                    cs += commit.subject.len();
                    cs += commit.body.as_ref().map_or(0, |b| b.len());
                    cs += commit
                        .parents
                        .iter()
                        .map(|p| std::mem::size_of::<String>() + p.len())
                        .sum::<usize>();
                    cs
                })
                .sum::<usize>();
            s
        })
        .sum::<usize>();

    size
}

use gitdiverge_lib::{Author, BranchComparison, Commit};

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn make_commit(hash: &str, subject: &str) -> Commit {
        Commit {
            hash: hash.to_string(),
            author: Author {
                name: "Test".to_string(),
                email: "test@example.com".to_string(),
            },
            timestamp: chrono::Utc::now(),
            subject: subject.to_string(),
            body: None,
            parents: Vec::new(),
        }
    }

    fn make_test_analytics() -> BranchAnalytics {
        BranchAnalytics {
            repo_path: "/tmp/repo".to_string(),
            branches: vec!["main".to_string(), "dev".to_string()],
            comparisons: vec![BranchComparison {
                source_branch: "dev".to_string(),
                target_branch: "main".to_string(),
                missing_commits: vec![make_commit("abc", "commit")],
            }],
        }
    }

    #[test]
    fn test_insert_and_get_with_branches() {
        let cache = DivergenceCache::new();
        let analytics = make_test_analytics();
        cache.insert_with_branches(
            "guid1".to_string(),
            &["main".to_string(), "dev".to_string()],
            analytics.clone(),
        );
        let cached = cache
            .get_with_branches("guid1", &["main".to_string(), "dev".to_string()])
            .unwrap();
        assert_eq!(cached.analytics.repo_path, "/tmp/repo");
        assert_eq!(cached.analytics.comparisons[0].missing_commits.len(), 1);
    }

    #[test]
    fn test_branch_order_normalised() {
        let cache = DivergenceCache::new();
        let analytics = make_test_analytics();
        cache.insert_with_branches(
            "guid1".to_string(),
            &["dev".to_string(), "main".to_string()],
            analytics.clone(),
        );
        // Lookup with different order should find the same entry because key sorts branches
        let cached = cache
            .get_with_branches("guid1", &["main".to_string(), "dev".to_string()])
            .unwrap();
        assert_eq!(cached.analytics.comparisons[0].missing_commits.len(), 1);
    }

    #[test]
    fn test_different_branches_miss() {
        let cache = DivergenceCache::new();
        let analytics = make_test_analytics();
        cache.insert_with_branches(
            "guid1".to_string(),
            &["main".to_string(), "dev".to_string()],
            analytics,
        );
        assert!(cache
            .get_with_branches("guid1", &["main".to_string(), "feature".to_string()])
            .is_none());
    }

    #[test]
    fn test_invalidate_repo() {
        let cache = DivergenceCache::new();
        let analytics = make_test_analytics();
        cache.insert_with_branches(
            "guid1".to_string(),
            &["main".to_string(), "dev".to_string()],
            analytics.clone(),
        );
        cache.insert_with_branches(
            "guid2".to_string(),
            &["main".to_string(), "dev".to_string()],
            analytics,
        );
        cache.invalidate_repo("guid1");
        assert!(cache
            .get_with_branches("guid1", &["main".to_string(), "dev".to_string()])
            .is_none());
        assert!(cache
            .get_with_branches("guid2", &["main".to_string(), "dev".to_string()])
            .is_some());
    }

    #[test]
    fn test_invalidate_all() {
        let cache = DivergenceCache::new();
        cache.insert_with_branches(
            "a".to_string(),
            &["main".to_string()],
            make_test_analytics(),
        );
        cache.insert_with_branches(
            "b".to_string(),
            &["main".to_string()],
            make_test_analytics(),
        );
        cache.invalidate_all();
        assert!(cache
            .get_with_branches("a", &["main".to_string()])
            .is_none());
        assert!(cache
            .get_with_branches("b", &["main".to_string()])
            .is_none());
    }

    #[test]
    fn test_concurrent_reads() {
        let cache = DivergenceCache::new();
        cache.insert_with_branches(
            "guid1".to_string(),
            &["main".to_string()],
            make_test_analytics(),
        );
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let cache = cache.clone();
                std::thread::spawn(move || {
                    assert!(cache
                        .get_with_branches("guid1", &["main".to_string()])
                        .is_some());
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_concurrent_write_read() {
        let cache = DivergenceCache::new();
        let write_handle = {
            let cache = cache.clone();
            std::thread::spawn(move || {
                cache.insert_with_branches(
                    "guid1".to_string(),
                    &["main".to_string()],
                    make_test_analytics(),
                );
            })
        };
        let read_handle = {
            let cache = cache.clone();
            std::thread::spawn(move || {
                // Timing-dependent: may be None or Some; either is valid.
                let _ = cache.get_with_branches("guid1", &["main".to_string()]);
            })
        };
        write_handle.join().unwrap();
        read_handle.join().unwrap();
    }

    #[test]
    fn test_eviction_removes_oldest_first() {
        let entry = make_test_analytics();
        let size = estimated_size(&entry);
        // Allow two entries but not three.
        let cache = DivergenceCache::with_max_bytes(size * 2 + 1);

        cache.insert_with_branches("guid1".to_string(), &["main".to_string()], entry.clone());
        thread::sleep(Duration::from_millis(10));
        cache.insert_with_branches("guid2".to_string(), &["main".to_string()], entry.clone());
        thread::sleep(Duration::from_millis(10));
        cache.insert_with_branches("guid3".to_string(), &["main".to_string()], entry);

        // Oldest entry must have been evicted to stay under the limit.
        assert!(
            cache
                .get_with_branches("guid1", &["main".to_string()])
                .is_none(),
            "oldest entry should have been evicted"
        );
        assert!(
            cache
                .get_with_branches("guid3", &["main".to_string()])
                .is_some(),
            "newest entry should still be present"
        );
        assert!(
            cache.len() <= 2,
            "expected at most 2 entries, got {}",
            cache.len()
        );
    }

    #[test]
    fn test_total_size_tracked_across_operations() {
        let cache = DivergenceCache::new();
        let analytics = make_test_analytics();
        let size_one = estimated_size(&analytics);

        cache.insert_with_branches(
            "guid1".to_string(),
            &["main".to_string()],
            analytics.clone(),
        );
        assert_eq!(cache.total_size(), size_one);

        cache.insert_with_branches(
            "guid2".to_string(),
            &["main".to_string()],
            analytics.clone(),
        );
        assert_eq!(cache.total_size(), size_one * 2);

        cache.invalidate_repo("guid1");
        assert_eq!(cache.total_size(), size_one);

        cache.invalidate_all();
        assert_eq!(cache.total_size(), 0);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_replace_entry_updates_size() {
        let cache = DivergenceCache::new();
        let small = BranchAnalytics {
            repo_path: "/a".to_string(),
            branches: vec!["main".to_string()],
            comparisons: vec![],
        };
        let large = make_test_analytics();
        let small_size = estimated_size(&small);
        let large_size = estimated_size(&large);

        cache.insert_with_branches("guid1".to_string(), &["main".to_string()], small);
        assert_eq!(cache.total_size(), small_size);

        cache.insert_with_branches("guid1".to_string(), &["main".to_string()], large);
        assert_eq!(cache.total_size(), large_size);
    }
}
