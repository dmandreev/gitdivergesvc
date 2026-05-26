use crate::error::Error;
use crate::git::ProcessGitProvider;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const INDEX_DIR: &str = ".gitdiverge";
const INDEX_FILE: &str = "index.json";

/// A single entry in the repository index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    pub guid: String,
    pub url: String,
    pub name: String,
    pub cloned_at: DateTime<Utc>,
}

/// Result of scanning a directory for repositories.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub guid: String,
    pub url: String,
    pub name: String,
    pub action: ScanAction,
}

/// What happened to a repository during a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanAction {
    /// Newly added to the index.
    Added,
    /// GUID changed and was updated in the index.
    Updated,
    /// Already in the index and matches on-disk state.
    Verified,
    /// Could not be processed.
    Skipped,
}

/// Registry that maps repository URLs to their local GUID-based clone directories.
///
/// The index is stored as JSON inside `<clone_dir>/.gitdiverge/index.json`.
/// Each repository is cloned into `<clone_dir>/<guid>/`.
#[derive(Debug, Clone)]
pub struct RepoIndex {
    clone_dir: PathBuf,
    entries: HashMap<String, RepoEntry>,
}

impl RepoIndex {
    /// Return the base clone directory.
    pub fn clone_dir(&self) -> &Path {
        &self.clone_dir
    }

    /// Open an existing index or create a new one for the given clone directory.
    pub fn open(clone_dir: &Path) -> Result<Self, Error> {
        let index_path = Self::index_path(clone_dir);
        let entries = if index_path.exists() {
            let content = std::fs::read_to_string(&index_path).map_err(Error::Io)?;
            let entries: HashMap<String, RepoEntry> = serde_json::from_str(&content)
                .map_err(|e| Error::Parse(format!("failed to parse repo index: {e}")))?;
            entries
        } else {
            HashMap::new()
        };

        Ok(Self {
            clone_dir: clone_dir.to_path_buf(),
            entries,
        })
    }

    /// Persist the index to disk atomically.
    ///
    /// Writes to a temporary file and renames it into place so concurrent
    /// readers never observe a partially-written index.
    pub fn save(&self) -> Result<(), Error> {
        let index_dir = self.clone_dir.join(INDEX_DIR);
        std::fs::create_dir_all(&index_dir).map_err(Error::Io)?;
        let index_path = index_dir.join(INDEX_FILE);
        let temp_path = index_dir.join(format!("{}.tmp", INDEX_FILE));
        let json = serde_json::to_string_pretty(&self.entries)
            .expect("RepoEntry Vec should always serialize");
        std::fs::write(&temp_path, json).map_err(Error::Io)?;
        std::fs::rename(&temp_path, &index_path).map_err(Error::Io)?;
        Ok(())
    }

    /// Look up a repository by its normalized URL.
    pub fn resolve(&self, url: &str) -> Option<&RepoEntry> {
        let key = normalize_url(url);
        self.entries.get(&key)
    }

    /// Look up a repository by its GUID.
    pub fn resolve_by_guid(&self, guid: &str) -> Option<&RepoEntry> {
        self.entries.values().find(|e| e.guid == guid)
    }

    /// Look up a repository by its human-readable name.
    ///
    /// **Note:** names are not guaranteed to be unique. This returns the first match.
    pub fn resolve_by_name(&self, name: &str) -> Option<&RepoEntry> {
        self.entries.values().find(|e| e.name == name)
    }

    /// Get or create an entry for the given URL.
    ///
    /// If the URL already exists in the index, the existing entry is returned.
    /// Otherwise a new GUID is generated and a new entry is inserted.
    pub fn get_or_insert(&mut self, url: String, name: String) -> &RepoEntry {
        let key = normalize_url(&url);
        if !self.entries.contains_key(&key) {
            let entry = RepoEntry {
                guid: uuid::Uuid::new_v4().to_string(),
                url: url.clone(),
                name,
                cloned_at: Utc::now(),
            };
            self.entries.insert(key.clone(), entry);
        }
        self.entries.get(&key).unwrap()
    }

    /// Return the filesystem path for a given entry.
    pub fn entry_path(&self, entry: &RepoEntry) -> PathBuf {
        self.clone_dir.join(&entry.guid).join(&entry.name)
    }

    /// Return the path that a new entry for the given URL would occupy.
    pub fn path_for_url(&self, url: &str) -> Option<PathBuf> {
        self.resolve(url).map(|e| self.entry_path(e))
    }

    /// Iterate over all entries.
    pub fn entries(&self) -> impl Iterator<Item = &RepoEntry> {
        self.entries.values()
    }

    /// Remove an entry by GUID.
    ///
    /// Returns the removed entry if it existed, or `None` if no entry with the
    /// given GUID was found. The index is **not** automatically persisted;
    /// callers must call [`save`](Self::save) afterwards.
    pub fn remove_by_guid(&mut self, guid: &str) -> Option<RepoEntry> {
        let key = self.entries.iter()
            .find(|(_, e)| e.guid == guid)
            .map(|(k, _)| k.clone())?;
        self.entries.remove(&key)
    }

    /// Scan the clone directory for repositories that are not in the index,
    /// or verify existing ones.
    ///
    /// Repositories are expected under `<clone_dir>/<guid>/<name>/`. For every
    /// such directory that contains a `.git` folder:
    /// - Read the `origin` remote URL.
    /// - If the URL is not in the index, add it.
    /// - If the URL is in the index but with a different GUID or name, update
    ///   the entry to match the on-disk layout (the on-disk layout wins).
    /// - If the GUID and name already match, verify the URL.
    pub fn scan(&mut self, git: &ProcessGitProvider) -> Result<Vec<ScanResult>, Error> {
        let mut results = Vec::new();

        let guid_read_dir = match std::fs::read_dir(&self.clone_dir) {
            Ok(rd) => rd,
            Err(e) => {
                tracing::warn!("failed to read clone_dir: {e}");
                return Ok(results);
            }
        };

        for guid_entry in guid_read_dir.flatten() {
            let guid_path = guid_entry.path();
            if !guid_path.is_dir() || guid_path.file_name() == Some(std::ffi::OsStr::new(INDEX_DIR))
            {
                continue;
            }

            let guid = guid_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let name_read_dir = match std::fs::read_dir(&guid_path) {
                Ok(rd) => rd,
                Err(e) => {
                    tracing::warn!(path = %guid_path.display(), error = %e, "failed to read guid directory");
                    continue;
                }
            };

            for name_entry in name_read_dir.flatten() {
                let repo_path = name_entry.path();
                if !repo_path.is_dir() {
                    continue;
                }
                if !repo_path.join(".git").is_dir() {
                    continue;
                }

                let name = repo_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let origin_url = match git.get_remote_url(&repo_path, "origin") {
                    Ok(url) => url,
                    Err(e) => {
                        tracing::warn!(path = %repo_path.display(), error = %e, "failed to read remote url");
                        continue;
                    }
                };

                let normalized = normalize_url(&origin_url);
                let action = if let Some(existing) = self.entries.get(&normalized) {
                    if existing.guid == guid && existing.name == name {
                        ScanAction::Verified
                    } else {
                        // On-disk layout wins — update index to match reality
                        let mut updated = existing.clone();
                        updated.guid = guid.clone();
                        updated.url = origin_url.clone();
                        updated.name = name.clone();
                        self.entries.insert(normalized, updated);
                        ScanAction::Updated
                    }
                } else {
                    // New repo found on disk
                    let name = repo_name_from_url(&origin_url);
                    self.entries.insert(
                        normalized,
                        RepoEntry {
                            guid: guid.clone(),
                            url: origin_url.clone(),
                            name,
                            cloned_at: Utc::now(),
                        },
                    );
                    ScanAction::Added
                };

                results.push(ScanResult {
                    guid: guid.clone(),
                    url: origin_url.clone(),
                    name: self
                        .entries
                        .get(&normalize_url(&origin_url))
                        .map(|e| e.name.clone())
                        .unwrap_or_else(|| repo_name_from_url(&origin_url)),
                    action,
                });
            }
        }

        Ok(results)
    }

    fn index_path(clone_dir: &Path) -> PathBuf {
        clone_dir.join(INDEX_DIR).join(INDEX_FILE)
    }
}

/// Normalize a URL so it can be used as a stable lookup key.
///
/// Rules:
/// 1. Trim trailing `.git`
/// 2. Trim trailing `/`
/// 3. Lowercase the scheme and host only (preserve path case)
pub fn normalize_url(url: &str) -> String {
    let url = url.trim();
    let mut url = url.strip_suffix(".git").unwrap_or(url);
    url = url.strip_suffix('/').unwrap_or(url);

    // Lowercase scheme and host
    if let Some(pos) = url.find("://") {
        let scheme = &url[..pos + 3];
        let rest = &url[pos + 3..];
        if let Some(slash_pos) = rest.find('/') {
            let host = &rest[..slash_pos];
            let path = &rest[slash_pos..];
            format!("{}{}{}", scheme.to_lowercase(), host.to_lowercase(), path)
        } else {
            format!("{}{}", scheme.to_lowercase(), rest.to_lowercase())
        }
    } else if let Some(at_pos) = url.find('@') {
        // SSH style: git@github.com:user/repo
        let before = &url[..at_pos + 1];
        let rest = &url[at_pos + 1..];
        if let Some(colon_pos) = rest.find(':') {
            let host = &rest[..colon_pos];
            let path = &rest[colon_pos..];
            format!("{}{}{}", before, host.to_lowercase(), path)
        } else {
            format!("{}{}", before, rest.to_lowercase())
        }
    } else {
        url.to_string()
    }
}

fn repo_name_from_url(url: &str) -> String {
    let name = url.rsplit('/').next().unwrap_or("repo");
    if name.is_empty() {
        "repo".to_string()
    } else {
        name.trim_end_matches(".git").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_variants() {
        assert_eq!(
            normalize_url("https://github.com/User/Repo.git"),
            "https://github.com/User/Repo"
        );
        assert_eq!(
            normalize_url("https://github.com/User/Repo"),
            "https://github.com/User/Repo"
        );
        assert_eq!(
            normalize_url("HTTPS://GITHUB.COM/User/Repo/"),
            "https://github.com/User/Repo"
        );
        assert_eq!(
            normalize_url("git@github.com:User/Repo.git"),
            "git@github.com:User/Repo"
        );
        // URL with scheme but no path (line 261).
        assert_eq!(normalize_url("HTTPS://EXAMPLE.COM"), "https://example.com");
        // SSH URL with @ but no colon (line 272).
        assert_eq!(normalize_url("git@EXAMPLE.COM"), "git@example.com");
        // Plain URL with no scheme and no @ (line 291-293 fallthrough).
        assert_eq!(
            normalize_url("github.com/User/Repo"),
            "github.com/User/Repo"
        );
    }

    #[test]
    fn get_or_insert_generates_unique_guids() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let guid1 = {
            let e1 = index.get_or_insert("https://a.com/r1".to_string(), "r1".to_string());
            e1.guid.clone()
        };
        let guid2 = {
            let e2 = index.get_or_insert("https://a.com/r2".to_string(), "r2".to_string());
            e2.guid.clone()
        };
        assert_ne!(guid1, guid2);
    }

    #[test]
    fn save_persists_and_reloads() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        index.get_or_insert("https://a.com/r1".to_string(), "r1".to_string());
        index.save().unwrap();

        let index2 = RepoIndex::open(tmp.path()).unwrap();
        assert!(index2.resolve("https://a.com/r1").is_some());
    }

    #[test]
    fn resolve_methods() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let entry = index.get_or_insert("https://a.com/r1".to_string(), "r1".to_string());
        let guid = entry.guid.clone();

        assert!(index.resolve("https://a.com/r1").is_some());
        assert!(index.resolve_by_guid(&guid).is_some());
        assert!(index.resolve_by_name("r1").is_some());
        assert!(index.resolve("https://a.com/missing").is_none());
        assert!(index.resolve_by_guid("no-such-guid").is_none());
        assert!(index.resolve_by_name("missing").is_none());

        let path = index.entry_path(index.resolve("https://a.com/r1").unwrap());
        assert_eq!(path, tmp.path().join(&guid).join("r1"));

        let path_opt = index.path_for_url("https://a.com/r1");
        assert_eq!(path_opt, Some(tmp.path().join(&guid).join("r1")));
        assert!(index.path_for_url("https://a.com/missing").is_none());

        assert_eq!(index.entries().count(), 1);
    }

    #[test]
    fn scan_finds_and_verifies_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let git = ProcessGitProvider::new();

        // Create a repo inside a GUID/name nested subdirectory
        let guid = "repo-guid-123";
        let repo_path = tmp.path().join(guid).join("repo");
        std::fs::create_dir_all(&repo_path).unwrap();

        std::process::Command::new("git")
            .arg("init")
            .arg(&repo_path)
            .status()
            .unwrap();
        std::fs::write(repo_path.join("file.txt"), "hello").unwrap();
        std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&repo_path)
            .args([
                "remote",
                "add",
                "origin",
                "https://example.com/owner/repo.git",
            ])
            .status()
            .unwrap();

        // Pre-populate index with matching URL but same GUID -> Verified
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        index.get_or_insert(
            "https://example.com/owner/repo.git".to_string(),
            "repo".to_string(),
        );
        // Force the GUID to match directory
        let key = normalize_url("https://example.com/owner/repo.git");
        index.entries.get_mut(&key).unwrap().guid = guid.to_string();
        index.save().unwrap();

        let results = index.scan(&git).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, ScanAction::Verified);

        // Now change GUID in index so disk wins -> Updated
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        index.entries.get_mut(&key).unwrap().guid = "old-guid".to_string();
        let results = index.scan(&git).unwrap();
        assert_eq!(results[0].action, ScanAction::Updated);
        assert_eq!(index.resolve_by_guid(guid).unwrap().guid, guid);
    }

    #[test]
    fn scan_adds_new_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let git = ProcessGitProvider::new();

        let guid = "new-repo-456";
        let repo_path = tmp.path().join(guid).join("newrepo");
        std::fs::create_dir_all(&repo_path).unwrap();

        std::process::Command::new("git")
            .arg("init")
            .arg(&repo_path)
            .status()
            .unwrap();
        std::fs::write(repo_path.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&repo_path)
            .args([
                "remote",
                "add",
                "origin",
                "https://example.com/owner/newrepo.git",
            ])
            .status()
            .unwrap();

        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let results = index.scan(&git).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, ScanAction::Added);
        assert!(index
            .resolve("https://example.com/owner/newrepo.git")
            .is_some());
    }

    #[test]
    fn scan_skips_non_repos_and_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let git = ProcessGitProvider::new();

        // Create a file (not a dir)
        std::fs::write(tmp.path().join("not-a-dir"), "x").unwrap();
        // Create a dir without .git
        std::fs::create_dir_all(tmp.path().join("no-git")).unwrap();
        // Create .gitdiverge dir
        std::fs::create_dir_all(tmp.path().join(".gitdiverge")).unwrap();

        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let results = index.scan(&git).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn scan_skips_file_and_non_git_inside_guid_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let git = ProcessGitProvider::new();

        let guid = "guid-789";
        let guid_path = tmp.path().join(guid);
        std::fs::create_dir_all(&guid_path).unwrap();

        // A file inside the GUID dir (not a repo directory) – covers line 189.
        std::fs::write(guid_path.join("readme.txt"), "hello").unwrap();
        // A directory inside the GUID dir without .git – covers line 192.
        std::fs::create_dir_all(guid_path.join("no-git")).unwrap();

        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let results = index.scan(&git).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn scan_handles_read_dir_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let git = ProcessGitProvider::new();

        // Use a file as clone_dir so read_dir fails
        let file_path = tmp.path().join("afile");
        std::fs::write(&file_path, "x").unwrap();

        let mut index = RepoIndex::open(&file_path).unwrap();
        let results = index.scan(&git).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn repo_name_from_url_empty_after_slash() {
        // Trailing slash leads to empty rsplit component (line 282).
        assert_eq!(repo_name_from_url("https://github.com/user/repo/"), "repo");
    }

    #[test]
    fn remove_by_guid_deletes_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let entry = index.get_or_insert("https://a.com/r1".to_string(), "r1".to_string());
        let guid = entry.guid.clone();

        assert!(index.resolve_by_guid(&guid).is_some());
        let removed = index.remove_by_guid(&guid);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().guid, guid);
        assert!(index.resolve_by_guid(&guid).is_none());
        assert_eq!(index.entries().count(), 0);
    }

    #[test]
    fn remove_by_guid_returns_none_for_missing_guid() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        assert!(index.remove_by_guid("no-such-guid").is_none());
    }

    #[test]
    fn scan_handles_get_remote_url_error() {
        let tmp = tempfile::tempdir().unwrap();
        let git = ProcessGitProvider::new();

        // Create a repo without any remote so get_remote_url fails.
        let guid = "no-remote-123";
        let repo_path = tmp.path().join(guid).join("repo");
        std::fs::create_dir_all(&repo_path).unwrap();
        std::process::Command::new("git")
            .arg("init")
            .arg(&repo_path)
            .status()
            .unwrap();
        std::fs::write(repo_path.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();

        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let results = index.scan(&git).unwrap();
        assert!(results.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn scan_handles_name_read_dir_failure() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let git = ProcessGitProvider::new();

        // Create a GUID directory that is not readable so name_read_dir fails.
        let guid = "unreadable-123";
        let guid_path = tmp.path().join(guid);
        std::fs::create_dir_all(&guid_path).unwrap();
        std::fs::write(guid_path.join("placeholder"), "x").unwrap();

        let mut perms = std::fs::metadata(&guid_path).unwrap().permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(&guid_path, perms).unwrap();

        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let results = index.scan(&git).unwrap();
        assert!(results.is_empty());

        // Restore permissions so tempfile cleanup succeeds.
        let mut perms = std::fs::metadata(&guid_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&guid_path, perms).unwrap();
    }
}
