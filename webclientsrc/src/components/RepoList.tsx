import { useEffect, useRef, useState, useMemo, useCallback } from 'react'
import { GitFork, RefreshCw, ChevronRight, HardDrive, Search, X, Trash2, AlertTriangle } from 'lucide-react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { listRepos, deleteRepo, type RepoIndexEntry } from '../api/client'
import { useAuth } from '../auth/AuthProvider'

interface RepoListProps {
  onSelect: (repo: RepoIndexEntry) => void
  selectedGuid?: string
  refreshSignal?: number
  onDelete?: (repo: RepoIndexEntry) => void
}

export function RepoList({ onSelect, selectedGuid, refreshSignal, onDelete }: RepoListProps) {
  const { getAccessToken } = useAuth()
  const [repos, setRepos] = useState<RepoIndexEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [filter, setFilter] = useState('')
  const [confirmRepo, setConfirmRepo] = useState<RepoIndexEntry | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [deleteError, setDeleteError] = useState<string | null>(null)
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

  const handleDeleteClick = useCallback((e: React.MouseEvent, repo: RepoIndexEntry) => {
    e.stopPropagation()
    setConfirmRepo(repo)
    setDeleteError(null)
  }, [])

  const handleConfirmDelete = useCallback(async () => {
    if (!confirmRepo) return
    setDeleting(true)
    setDeleteError(null)
    try {
      const token = getAccessToken()
      const { error: apiError } = await deleteRepo({
        path: { repo_guid: confirmRepo.guid },
        auth: token ?? undefined,
      })
      if (apiError) throw apiError
      setRepos((prev) => prev.filter((r) => r.guid !== confirmRepo.guid))
      onDelete?.(confirmRepo)
      setConfirmRepo(null)
    } catch (e) {
      setDeleteError(e instanceof Error ? e.message : 'Failed to delete repository')
    } finally {
      setDeleting(false)
    }
  }, [confirmRepo, getAccessToken, onDelete])

  const handleCancelDelete = useCallback(() => {
    setConfirmRepo(null)
    setDeleteError(null)
  }, [])

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
                <div
                  key={repo.guid}
                  role="button"
                  tabIndex={0}
                  onClick={() => onSelect(repo)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault()
                      onSelect(repo)
                    }
                  }}
                  className={`w-full text-left px-3 py-2.5 rounded-lg border transition-all duration-200 group cursor-pointer
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
                    <div className="flex items-center gap-1 shrink-0">
                      <button
                        onClick={(e) => handleDeleteClick(e, repo)}
                        className="p-1.5 rounded-md text-text-dim hover:text-red hover:bg-red/10 transition-colors opacity-0 group-hover:opacity-100 focus:opacity-100"
                        aria-label={`Delete repository ${repo.name}`}
                        title="Delete repository"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                      <ChevronRight
                        className={`w-4 h-4 shrink-0 transition-colors ${
                          isActive ? 'text-accent-light' : 'text-text-dim'
                        }`}
                      />
                    </div>
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      )}

      {/* Delete confirmation modal */}
      {confirmRepo && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
          onClick={handleCancelDelete}
          role="presentation"
        >
          <div
            className="w-full max-w-sm mx-4 p-5 rounded-xl bg-surface-1 border border-surface-3 shadow-2xl space-y-4"
            onClick={(e) => e.stopPropagation()}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="delete-repo-title"
            aria-describedby="delete-repo-desc"
          >
            <div className="flex items-start gap-3">
              <div className="w-9 h-9 rounded-lg bg-red/10 flex items-center justify-center shrink-0">
                <AlertTriangle className="w-5 h-5 text-red" />
              </div>
              <div className="space-y-1">
                <h3 id="delete-repo-title" className="text-sm font-semibold text-text-main">
                  Delete repository
                </h3>
                <p id="delete-repo-desc" className="text-xs text-text-muted leading-relaxed">
                  Are you sure you want to delete <strong className="text-text-main">{confirmRepo.name}</strong>?
                  This will remove the local clone and index entry. This action cannot be undone.
                </p>
              </div>
            </div>

            {deleteError && (
              <div className="px-3 py-2 rounded-lg bg-red/10 border border-red/20 text-xs text-red">
                {deleteError}
              </div>
            )}

            <div className="flex items-center justify-end gap-2">
              <button
                onClick={handleCancelDelete}
                disabled={deleting}
                className="px-3 py-1.5 rounded-md text-xs font-medium text-text-main bg-surface-2 hover:bg-surface-3 transition-colors disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                onClick={handleConfirmDelete}
                disabled={deleting}
                className="px-3 py-1.5 rounded-md text-xs font-medium text-white bg-red hover:bg-red/90 transition-colors disabled:opacity-50 flex items-center gap-1.5"
              >
                {deleting && <RefreshCw className="w-3 h-3 animate-spin" />}
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
