import { useEffect, useRef, useState, useMemo, useCallback } from 'react'
import { GitFork, RefreshCw, ChevronRight, HardDrive, Search, X } from 'lucide-react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { listRepos, type RepoIndexEntry } from '../api/client'
import { useAuth } from '../auth/AuthProvider'

interface RepoListProps {
  onSelect: (repo: RepoIndexEntry) => void
  selectedGuid?: string
  refreshSignal?: number
}

export function RepoList({ onSelect, selectedGuid, refreshSignal }: RepoListProps) {
  const { getAccessToken } = useAuth()
  const [repos, setRepos] = useState<RepoIndexEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [filter, setFilter] = useState('')
  const requestIdRef = useRef(0)

  const load = useCallback(async () => {
    const reqId = ++requestIdRef.current
    setLoading(true)
    setError(null)
    try {
      const token = getAccessToken()
      const { data, error: apiError } = await listRepos({
        headers: token ? { Authorization: `Bearer ${token}` } : undefined,
      })
      if (reqId !== requestIdRef.current) return
      if (apiError) throw apiError
      setRepos(data as RepoIndexEntry[])
    } catch (e) {
      if (reqId !== requestIdRef.current) return
      setError(e instanceof Error ? e.message : 'Failed to load repositories')
    } finally {
      if (reqId === requestIdRef.current) {
        setLoading(false)
      }
    }
  }, [getAccessToken])

  useEffect(() => {
    load()
  }, [load])

  useEffect(() => {
    if (refreshSignal !== undefined && refreshSignal > 0) {
      load()
    }
  }, [refreshSignal, load])

  const sortedRepos = useMemo(
    () => [...repos].sort((a, b) => a.name.localeCompare(b.name)),
    [repos]
  )

  const filteredRepos = useMemo(() => {
    const term = filter.trim().toLowerCase()
    if (!term) return sortedRepos
    return sortedRepos.filter(
      (r) =>
        r.name.toLowerCase().includes(term) ||
        r.url.toLowerCase().includes(term)
    )
  }, [sortedRepos, filter])

  const parentRef = useRef<HTMLDivElement>(null)
  const virtualizer = useVirtualizer({
    count: filteredRepos.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 56,
    overscan: 5,
  })

  const virtualItems = virtualizer.getVirtualItems()

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold text-text-muted uppercase tracking-wider">
          Repositories
        </h2>
        <div className="flex items-center gap-2">
          <button
            onClick={load}
            disabled={loading}
            className="p-1.5 rounded-md text-text-dim hover:text-text-main hover:bg-surface-2 transition-colors disabled:opacity-50"
            aria-label="Refresh repositories"
          >
            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
          </button>
        </div>
      </div>

      <div className="relative">
        <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-text-dim pointer-events-none" />
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter repositories..."
          className="w-full pl-8 pr-7 py-1.5 rounded-md bg-surface-1 border border-surface-3 text-sm text-text-main placeholder:text-text-dim focus:outline-none focus:border-accent/60 transition-colors"
        />
        {filter && (
          <button
            onClick={() => setFilter('')}
            className="absolute right-2 top-1/2 -translate-y-1/2 text-text-dim hover:text-text-main"
            aria-label="Clear filter"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        )}
      </div>

      {error && (
        <div className="px-4 py-3 rounded-lg bg-red/10 border border-red/20 text-xs text-red">
          {error}
        </div>
      )}

      {repos.length === 0 && !loading && !error && (
        <div className="px-4 py-8 rounded-xl border border-dashed border-surface-3 text-center space-y-2">
          <HardDrive className="w-8 h-8 text-text-dim mx-auto" />
          <p className="text-sm text-text-muted">No repositories indexed yet.</p>
          <p className="text-xs text-text-dim">
            Clone a repo from the main panel to get started.
          </p>
        </div>
      )}

      {filteredRepos.length === 0 && repos.length > 0 && !loading && (
        <div className="px-4 py-6 rounded-xl border border-dashed border-surface-3 text-center">
          <p className="text-sm text-text-muted">No repositories match your filter.</p>
        </div>
      )}

      {filteredRepos.length > 0 && (
        <div
          ref={parentRef}
          className="max-h-[calc(100vh-14rem)] overflow-y-auto pr-1"
        >
          <div
            style={{
              height: `${virtualizer.getTotalSize()}px`,
              width: '100%',
              position: 'relative',
            }}
          >
            {virtualItems.map((virtualItem) => {
              const repo = filteredRepos[virtualItem.index]
              const isActive = repo.guid === selectedGuid
              return (
                <button
                  key={repo.guid}
                  onClick={() => onSelect(repo)}
                  className={`w-full text-left px-3 py-2.5 rounded-lg border transition-all duration-200 group
                    ${
                      isActive
                        ? 'bg-surface-2 border-accent/40 shadow-lg shadow-accent-glow/20'
                        : 'bg-surface-1/60 border-transparent hover:border-surface-3 hover:bg-surface-2'
                    }`}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    height: `${virtualItem.size}px`,
                    transform: `translateY(${virtualItem.start}px)`,
                  }}
                >
                  <div className="flex items-center justify-between h-full">
                    <div className="flex items-center gap-2.5 min-w-0">
                      <div
                        className={`w-7 h-7 rounded-lg flex items-center justify-center shrink-0 ${
                          isActive
                            ? 'bg-accent/20 text-accent-light'
                            : 'bg-surface-2 text-text-dim group-hover:text-text-muted'
                        }`}
                      >
                        <GitFork className="w-3.5 h-3.5" />
                      </div>
                      <div className="min-w-0">
                        <p className="text-sm font-medium text-text-main truncate">
                          {repo.name}
                        </p>
                        <p className="text-[11px] text-text-dim truncate">{repo.url}</p>
                      </div>
                    </div>
                    <ChevronRight
                      className={`w-4 h-4 shrink-0 transition-colors ${
                        isActive ? 'text-accent-light' : 'text-text-dim'
                      }`}
                    />
                  </div>
                </button>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}
