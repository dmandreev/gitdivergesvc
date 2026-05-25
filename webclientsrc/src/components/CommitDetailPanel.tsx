import { useMemo, useState, useCallback, useEffect, useRef } from 'react'
import { X, Search, GitCommit, User, Clock, Hash, ChevronRight, AlertCircle, Loader2 } from 'lucide-react'
import type { Commit } from '../generated/types.gen'
import { getDivergenceCommits } from '../api/client'
import { VirtualList } from './VirtualList'
import { JiraLinkText } from './JiraLinkText'

const DEFAULT_PANEL_WIDTH = 672
const MIN_PANEL_WIDTH = 320
const MAX_PANEL_WIDTH_FRACTION = 0.9
const STORAGE_KEY = 'gitdiverge-panel-width'
const PAGE_SIZE = 50

interface CommitDetailPanelProps {
  isOpen: boolean
  onClose: () => void
  repoGuid: string
  sourceBranch: string
  targetBranch: string
  branches: string[]
  totalCommits: number
}

function formatCommitDate(iso: string): string {
  const date = new Date(iso)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffSec = Math.floor(diffMs / 1000)
  const diffMin = Math.floor(diffSec / 60)
  const diffHour = Math.floor(diffMin / 60)
  const diffDay = Math.floor(diffHour / 24)

  if (diffDay > 365) return date.toLocaleDateString()
  if (diffDay > 30) return `${Math.floor(diffDay / 30)}mo ago`
  if (diffDay > 0) return `${diffDay}d ago`
  if (diffHour > 0) return `${diffHour}h ago`
  if (diffMin > 0) return `${diffMin}m ago`
  return 'just now'
}

function shortHash(hash: string): string {
  return hash.slice(0, 7)
}

function authorInitial(name: string): string {
  return name.charAt(0).toUpperCase()
}

const ITEM_HEIGHT = 76

export function CommitDetailPanel({
  isOpen,
  onClose,
  repoGuid,
  sourceBranch,
  targetBranch,
  branches,
  totalCommits,
}: CommitDetailPanelProps) {
  const [commits, setCommits] = useState<Commit[]>([])
  const [page, setPage] = useState(0)
  const [isLoading, setIsLoading] = useState(false)
  const [isLoadingMore, setIsLoadingMore] = useState(false)
  const [hasMore, setHasMore] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const [query, setQuery] = useState('')
  const [selectedHash, setSelectedHash] = useState<string | null>(null)
  const [panelWidth, setPanelWidth] = useState<number>(() => {
    try {
      const stored = window.localStorage.getItem(STORAGE_KEY)
      if (stored) {
        const parsed = parseInt(stored, 10)
        if (!Number.isNaN(parsed) && parsed >= MIN_PANEL_WIDTH) return parsed
      }
    } catch {
      // ignore
    }
    return DEFAULT_PANEL_WIDTH
  })
  const [isDragging, setIsDragging] = useState(false)
  const panelRef = useRef<HTMLDivElement>(null)
  const currentWidthRef = useRef(panelWidth)

  useEffect(() => {
    currentWidthRef.current = panelWidth
  }, [panelWidth])

  useEffect(() => {
    if (!isDragging) return

    const handleMouseMove = (e: MouseEvent) => {
      const newWidth = Math.max(
        MIN_PANEL_WIDTH,
        Math.min(
          window.innerWidth * MAX_PANEL_WIDTH_FRACTION,
          window.innerWidth - e.clientX
        )
      )
      if (panelRef.current) {
        panelRef.current.style.width = `${newWidth}px`
      }
      currentWidthRef.current = newWidth
    }

    const handleMouseUp = () => {
      setIsDragging(false)
      const finalWidth = currentWidthRef.current
      setPanelWidth(finalWidth)
      try {
        window.localStorage.setItem(STORAGE_KEY, String(finalWidth))
      } catch {
        // ignore
      }
    }

    document.addEventListener('mousemove', handleMouseMove)
    document.addEventListener('mouseup', handleMouseUp)
    document.body.style.cursor = 'ew-resize'
    document.body.style.userSelect = 'none'

    return () => {
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
  }, [isDragging])

  const fetchCommits = useCallback(
    async (pageNum: number, append: boolean) => {
      const loadingSetter = pageNum === 0 ? setIsLoading : setIsLoadingMore
      loadingSetter(true)
      setError(null)

      try {
        const response = await getDivergenceCommits({
          path: { repo_guid: repoGuid },
          query: {
            branches: branches.join(','),
            source_branch: sourceBranch,
            target_branch: targetBranch,
            page: pageNum,
            page_size: PAGE_SIZE,
          },
        })

        const data = response.data
        if (!data) {
          throw new Error('Empty response')
        }

        if (append) {
          setCommits((prev) => [...prev, ...data.commits])
        } else {
          setCommits(data.commits)
        }

        setHasMore(data.commits.length === PAGE_SIZE)
        setPage(pageNum)
      } catch (err) {
        const message = err instanceof Error ? err.message : 'Failed to load commits'
        if (message.includes('404') || message.includes('Not Found') || message.includes('not found')) {
          setError('Divergence data not found. Please run the analysis first.')
        } else {
          setError(message)
        }
      } finally {
        loadingSetter(false)
      }
    },
    [repoGuid, sourceBranch, targetBranch, branches]
  )

  useEffect(() => {
    if (!isOpen) return
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setCommits([])
    setPage(0)
    setHasMore(true)
    setError(null)
    setQuery('')
    setSelectedHash(null)
    fetchCommits(0, false)
  }, [isOpen, repoGuid, sourceBranch, targetBranch, fetchCommits])

  const filteredCommits = useMemo(() => {
    let result = [...commits]
    result.sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime())
    if (query.trim()) {
      const q = query.toLowerCase()
      result = result.filter(
        (c) =>
          c.subject.toLowerCase().includes(q) ||
          c.author.name.toLowerCase().includes(q) ||
          c.author.email.toLowerCase().includes(q) ||
          c.hash.toLowerCase().includes(q) ||
          (c.body?.toLowerCase().includes(q) ?? false)
      )
    }
    return result
  }, [commits, query])

  const selectedCommit = useMemo(
    () => commits.find((c) => c.hash === selectedHash) ?? null,
    [commits, selectedHash]
  )

  const handleSelect = useCallback((hash: string) => {
    setSelectedHash((prev) => (prev === hash ? null : hash))
  }, [])

  const handleClose = useCallback(() => {
    onClose()
    setQuery('')
    setSelectedHash(null)
  }, [onClose])

  const startResize = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setIsDragging(true)
  }, [])

  const displayTotal = totalCommits

  return (
    <div
      className={`fixed inset-0 z-50 transition-opacity duration-200 ${
        isOpen ? 'opacity-100' : 'opacity-0 pointer-events-none'
      }`}
      aria-hidden={!isOpen}
    >
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/60"
        onClick={handleClose}
        aria-hidden="true"
      />

      {/* Panel */}
      <div
        ref={panelRef}
        style={{ width: panelWidth }}
        className={`absolute right-0 top-0 bottom-0 bg-surface-1 border-l border-surface-3 shadow-2xl flex flex-col transition-transform duration-200 ${
          isOpen ? 'translate-x-0' : 'translate-x-full'
        }`}
        role="dialog"
        aria-modal="true"
        aria-label="Missing commits"
      >
        {/* Resize handle */}
        <div
          className="absolute left-0 top-0 bottom-0 w-4 -translate-x-1/2 cursor-ew-resize z-20 group flex justify-center"
          onMouseDown={startResize}
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize panel"
        >
          <div
            className={`w-0.5 h-full transition-colors ${
              isDragging ? 'bg-accent/50' : 'bg-surface-3/0 group-hover:bg-accent/30'
            }`}
          />
        </div>

        {/* Header */}
        <div className="shrink-0 px-5 py-4 border-b border-surface-3 flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold text-text-main">
              Missing commits
            </h2>
            <p className="text-[11px] text-text-muted mt-0.5">
              <span className="text-accent-light font-medium">{sourceBranch}</span>
              <ChevronRight className="w-3 h-3 inline mx-0.5 text-text-dim" />
              <span className="text-text-dim">{targetBranch}</span>
              <span className="mx-1.5 text-surface-3">·</span>
              <span className="font-mono">{displayTotal}</span>{' '}
              {displayTotal === 1 ? 'commit' : 'commits'}
            </p>
          </div>
          <button
            onClick={handleClose}
            className="shrink-0 p-1.5 rounded-md text-text-dim hover:text-text-main hover:bg-surface-2 transition-colors"
            aria-label="Close panel"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Search */}
        <div className="shrink-0 px-5 py-3 border-b border-surface-3">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-text-dim" />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filter by subject, author, hash…"
              className="w-full pl-9 pr-3 py-2 rounded-lg bg-surface-0 border border-surface-3 text-sm text-text-main placeholder:text-text-dim focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/30 transition-all"
            />
          </div>
          {query && commits.length < displayTotal && (
            <p className="text-[11px] text-text-dim mt-1.5">
              Showing results from {commits.length} loaded commits (of {displayTotal} total)
            </p>
          )}
        </div>

        {/* List */}
        <div className="flex-1 min-h-0 overflow-auto">
          {isLoading ? (
            <div className="flex flex-col items-center justify-center h-full gap-3 text-center px-6">
              <Loader2 className="w-8 h-8 text-text-dim animate-spin" />
              <p className="text-sm text-text-muted">Loading commits…</p>
            </div>
          ) : error ? (
            <div className="flex flex-col items-center justify-center h-full gap-3 text-center px-6">
              <AlertCircle className="w-8 h-8 text-red" />
              <p className="text-sm text-text-muted">{error}</p>
              <button
                onClick={() => fetchCommits(0, false)}
                className="px-3 py-1.5 rounded-lg text-xs font-medium bg-surface-2 hover:bg-surface-3 text-text-main transition-colors"
              >
                Retry
              </button>
            </div>
          ) : displayTotal === 0 ? (
            <div className="flex flex-col items-center justify-center h-full gap-3 text-center px-6">
              <GitCommit className="w-8 h-8 text-text-dim" />
              <p className="text-sm text-text-muted">
                No commits missing between these branches
              </p>
            </div>
          ) : filteredCommits.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-full gap-3 text-center px-6">
              <GitCommit className="w-8 h-8 text-text-dim" />
              <p className="text-sm text-text-muted">
                {query.trim() ? 'No commits match your filter.' : 'No commits on this page'}
              </p>
            </div>
          ) : (
            <VirtualList
              items={filteredCommits}
              itemHeight={ITEM_HEIGHT}
              className="h-full"
              renderItem={(commit) => (
                <CommitRow
                  commit={commit}
                  isActive={selectedHash === commit.hash}
                  onClick={() => handleSelect(commit.hash)}
                />
              )}
            />
          )}
        </div>

        {/* Load more */}
        {hasMore && !isLoading && !error && (
          <div className="shrink-0 px-5 py-3 border-t border-surface-3">
            {isLoadingMore ? (
              <div className="flex items-center justify-center gap-2 text-sm text-text-muted">
                <Loader2 className="w-4 h-4 animate-spin" />
                Loading more…
              </div>
            ) : (
              <button
                onClick={() => fetchCommits(page + 1, true)}
                className="w-full py-2 rounded-lg text-sm font-medium bg-surface-2 hover:bg-surface-3 text-text-main transition-colors"
              >
                Load more commits
              </button>
            )}
          </div>
        )}
      </div>

      {/* Commit detail modal */}
      {selectedCommit && (
        <div
          className="absolute inset-0 z-10 flex items-center justify-center p-4"
          onClick={() => setSelectedHash(null)}
          role="dialog"
          aria-modal="true"
          aria-label="Commit details"
        >
          <div
            className="bg-surface-1 rounded-xl shadow-2xl border border-surface-3 max-w-lg w-full max-h-[80vh] overflow-auto p-6"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-start justify-between gap-3 mb-4">
              <div className="min-w-0 flex-1">
                <p className="text-base font-semibold text-text-main leading-snug">
                  <JiraLinkText text={selectedCommit.subject} />
                </p>
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1 mt-2 text-xs text-text-muted">
                  <span className="inline-flex items-center gap-1">
                    <User className="w-3.5 h-3.5 text-text-dim" />
                    {selectedCommit.author.name}
                  </span>
                  <span className="inline-flex items-center gap-1">
                    <Clock className="w-3.5 h-3.5 text-text-dim" />
                    {new Date(selectedCommit.timestamp).toLocaleString()}
                  </span>
                  <span className="inline-flex items-center gap-1 font-mono">
                    <Hash className="w-3.5 h-3.5 text-text-dim" />
                    {selectedCommit.hash}
                  </span>
                </div>
              </div>
              <button
                onClick={() => setSelectedHash(null)}
                className="shrink-0 p-1.5 rounded-md text-text-dim hover:text-text-main hover:bg-surface-2 transition-colors"
                aria-label="Close commit details"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            {selectedCommit.body && (
              <pre className="text-xs text-text-muted whitespace-pre-wrap bg-surface-0 p-3 rounded-lg border border-surface-3 leading-relaxed">
                <JiraLinkText text={selectedCommit.body} />
              </pre>
            )}

            {selectedCommit.parents.length > 0 && (
              <div className="flex flex-wrap items-center gap-2 mt-4">
                <span className="text-xs text-text-dim">Parents:</span>
                {selectedCommit.parents.map((p) => (
                  <span
                    key={p}
                    className="text-xs font-mono bg-surface-2 px-1.5 py-0.5 rounded text-text-muted"
                  >
                    {shortHash(p)}
                  </span>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  )
}

interface CommitRowProps {
  commit: Commit
  isActive: boolean
  onClick: () => void
}

function CommitRow({ commit, isActive, onClick }: CommitRowProps) {
  return (
    <button
      onClick={onClick}
      className={`w-full text-left px-5 py-3 flex items-start gap-3 transition-colors border-b border-surface-3/50 ${
        isActive
          ? 'bg-accent/5'
          : 'hover:bg-surface-2/50'
      }`}
    >
      <div
        className={`w-7 h-7 rounded-full flex items-center justify-center text-xs font-bold shrink-0 mt-0.5 ${
          isActive ? 'bg-accent/20 text-accent-light' : 'bg-surface-2 text-text-muted'
        }`}
      >
        {authorInitial(commit.author.name)}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-[11px] font-mono bg-surface-2 px-1.5 py-0.5 rounded text-text-dim shrink-0">
            {shortHash(commit.hash)}
          </span>
          <span className="text-sm font-medium text-text-main truncate">
            <JiraLinkText text={commit.subject} />
          </span>
        </div>
        <div className="flex items-center gap-3 mt-1 text-[11px] text-text-muted">
          <span className="truncate">{commit.author.name}</span>
          <span className="text-text-dim">·</span>
          <span className="shrink-0">{formatCommitDate(commit.timestamp)}</span>
        </div>
      </div>
      {isActive && (
        <ChevronRight className="w-4 h-4 text-accent-light shrink-0 mt-1.5" />
      )}
    </button>
  )
}
