import { useState, useMemo, useCallback } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  Play,
  Square,
  AlertCircle,
  GitCommit,
  CheckCircle2,
  ArrowLeft,
  BarChart3,
  GitBranch,
  List,
} from 'lucide-react'
import { useAuth } from '../auth/AuthProvider'
import { CONFIG } from '../config'
import { useBatchDivergenceStream } from '../hooks/useBatchDivergenceStream'
import { BulkProgressPanel } from '../components/BulkProgressPanel'
import { DivergenceMatrix } from '../components/DivergenceMatrix'
import { CommitDetailPanel } from '../components/CommitDetailPanel'
import type { BatchDivergenceResponseItem, BranchStatus } from '../api/client'

const STORAGE_KEY_URLS = 'gitdiverge_bulk_urls'
const STORAGE_KEY_BRANCHES = 'gitdiverge_branches'
const DEFAULT_BRANCHES = 'main, develop, stage'

interface SelectedCell {
  repoGuid: string
  sourceBranch: string
  targetBranch: string
  commitCount: number
  branches: string[]
}

function parseUrls(value: string): string[] {
  return value
    .split('\n')
    .map((u) => u.trim())
    .filter(Boolean)
}

function parseBranches(value: string): string[] {
  return value
    .split(',')
    .map((b) => b.trim())
    .filter(Boolean)
}

export function BulkAnalysis() {
  const { getAccessToken } = useAuth()
  const token = getAccessToken()
  const navigate = useNavigate()
  const batchStream = useBatchDivergenceStream(token)

  const [urlsInput, setUrlsInput] = useState(() => {
    try {
      return localStorage.getItem(STORAGE_KEY_URLS) ?? ''
    } catch {
      return ''
    }
  })
  const [branchesInput, setBranchesInput] = useState(() => {
    try {
      return localStorage.getItem(STORAGE_KEY_BRANCHES) ?? DEFAULT_BRANCHES
    } catch {
      return DEFAULT_BRANCHES
    }
  })
  const [selectedCell, setSelectedCell] = useState<SelectedCell | null>(null)

  const urls = parseUrls(urlsInput)
  const branches = parseBranches(branchesInput)

  const isBusy =
    batchStream.state.status === 'connecting' || batchStream.state.status === 'streaming'

  const handleAnalyze = () => {
    if (urls.length === 0 || branches.length === 0) return
    const repos = urls.map((url) => ({ url }))
    batchStream.start(repos, branches)
  }

  const handleUrlsChange = (value: string) => {
    setUrlsInput(value)
    try {
      localStorage.setItem(STORAGE_KEY_URLS, value)
    } catch {
      // ignore
    }
  }

  const handleBranchesChange = (value: string) => {
    setBranchesInput(value)
    try {
      localStorage.setItem(STORAGE_KEY_BRANCHES, value)
    } catch {
      // ignore
    }
  }

  const orderedResults = useMemo<BatchDivergenceResponseItem[]>(() => {
    const result = batchStream.state.result
    if (!result) return []
    // Preserve input order using repo_url matching.
    const map = new Map<string, BatchDivergenceResponseItem>()
    for (const item of result) {
      map.set(item.repo_url, item)
    }
    const ordered: BatchDivergenceResponseItem[] = []
    for (const url of urls) {
      const item = map.get(url)
      if (item) {
        ordered.push(item)
        map.delete(url)
      }
    }
    // Append any remaining items that did not match (should not happen normally).
    for (const item of map.values()) {
      ordered.push(item)
    }
    return ordered
  }, [batchStream.state.result, urls])

  const needsToken = CONFIG.USE_AUTH && !token

  const handleCellClick = useCallback(
    (repoGuid: string, branchStatuses: BranchStatus[], source: string, target: string, count: number) => {
      if (count === 0) return
      const validBranches = branchStatuses.filter((bs) => bs.exists).map((bs) => bs.branch)
      setSelectedCell({
        repoGuid,
        sourceBranch: source,
        targetBranch: target,
        commitCount: count,
        branches: validBranches,
      })
    },
    []
  )

  return (
    <div className="min-h-screen bg-surface-0 text-text-main">
      <header className="border-b border-surface-3 bg-surface-1/80 backdrop-blur-md sticky top-0 z-50">
        <div className="max-w-6xl mx-auto px-6 h-16 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button
              onClick={() => navigate('/')}
              className="p-1.5 rounded-md text-text-dim hover:text-text-main hover:bg-surface-2 transition-colors"
              aria-label="Back to repositories"
            >
              <ArrowLeft className="w-4 h-4" />
            </button>
            <div className="w-9 h-9 rounded-xl bg-gradient-to-br from-accent to-accent-light flex items-center justify-center shadow-lg shadow-accent-glow">
              <BarChart3 className="w-5 h-5 text-white" />
            </div>
            <div>
              <h1 className="text-lg font-semibold tracking-tight text-text-main leading-tight">
                Bulk Analysis
              </h1>
              <p className="text-[11px] text-text-dim leading-tight tracking-wide uppercase">
                Multi-Repo Divergence
              </p>
            </div>
          </div>
        </div>
      </header>

      <main className="max-w-6xl mx-auto px-6 py-8 space-y-8">
        {/* Input Section */}
        <div className="space-y-6">
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
            {/* Repo URLs */}
            <div className="space-y-2">
              <label className="text-xs font-semibold text-text-muted uppercase tracking-wider flex items-center gap-2">
                <GitBranch className="w-3.5 h-3.5" />
                Repository URLs
              </label>
              <textarea
                value={urlsInput}
                onChange={(e) => handleUrlsChange(e.target.value)}
                placeholder={`https://github.com/org/repo-one.git\nhttps://github.com/org/repo-two.git`}
                rows={6}
                disabled={isBusy || needsToken}
                className="w-full px-3 py-2.5 rounded-lg bg-surface-1 border border-surface-3 text-sm text-text-main placeholder:text-text-dim focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/30 transition-all resize-y disabled:opacity-50 font-mono"
              />
              <p className="text-[11px] text-text-dim">
                One repository URL per line. Order determines report order.
              </p>
            </div>

            {/* Branches */}
            <div className="space-y-2">
              <label className="text-xs font-semibold text-text-muted uppercase tracking-wider flex items-center gap-2">
                <GitCommit className="w-3.5 h-3.5" />
                Branches to compare
              </label>
              <input
                type="text"
                value={branchesInput}
                onChange={(e) => handleBranchesChange(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    handleAnalyze()
                  }
                }}
                placeholder="main, develop, stage"
                disabled={isBusy || needsToken}
                className="w-full px-3 py-2.5 rounded-lg bg-surface-1 border border-surface-3 text-sm text-text-main placeholder:text-text-dim focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/30 transition-all disabled:opacity-50"
              />
              <p className="text-[11px] text-text-dim">
                Comma-separated branch names. The same branches are analyzed across all repositories.
              </p>
            </div>
          </div>

          <div className="flex items-center gap-3">
            {isBusy ? (
              <button
                onClick={() => batchStream.cancel()}
                className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-semibold text-red bg-red/10 hover:bg-red/20 border border-red/20 transition-colors"
              >
                <Square className="w-4 h-4 fill-current" />
                Cancel
              </button>
            ) : (
              <button
                onClick={handleAnalyze}
                disabled={needsToken || urls.length === 0 || branches.length === 0}
                className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-semibold text-white bg-gradient-to-r from-accent to-accent-light hover:opacity-90 disabled:opacity-40 transition-opacity shadow-lg shadow-accent-glow"
              >
                <Play className="w-4 h-4 fill-current" />
                Analyze {urls.length > 0 ? `${urls.length} repositories` : ''}
              </button>
            )}
            {needsToken && (
              <span className="text-xs text-text-muted">Authentication required to analyze.</span>
            )}
          </div>
        </div>

        {/* Progress */}
        {batchStream.state.progress && batchStream.state.status !== 'idle' && (
          <div className="p-4 rounded-xl bg-surface-1 border border-surface-3 space-y-4">
            <div className="flex items-center gap-2 text-xs text-text-muted font-medium">
              <GitCommit className="w-3.5 h-3.5 text-accent-light" />
              {batchStream.state.status === 'connecting'
                ? 'Connecting…'
                : batchStream.state.status === 'streaming'
                  ? 'Running bulk divergence analysis…'
                  : batchStream.state.status === 'complete'
                    ? 'Analysis complete'
                    : 'Analysis failed'}
            </div>
            <BulkProgressPanel
              progress={batchStream.state.progress}
              phase={batchStream.state.phase}
              phaseComplete={batchStream.state.phaseComplete}
              activeRepo={batchStream.state.activeRepo}
            />
          </div>
        )}

        {/* Error */}
        {batchStream.state.status === 'error' && batchStream.state.error && (
          <div className="p-4 rounded-xl bg-red/5 border border-red/20 space-y-2">
            <div className="flex items-center gap-2 text-sm font-semibold text-red">
              <AlertCircle className="w-4 h-4" />
              Bulk analysis failed
            </div>
            <p className="text-xs text-text-muted">{batchStream.state.error.message}</p>
            {batchStream.state.error.details && (
              <pre className="text-[11px] text-text-dim bg-surface-0 p-2 rounded-lg overflow-x-auto">
                {batchStream.state.error.details}
              </pre>
            )}
          </div>
        )}

        {/* Results */}
        {batchStream.state.status === 'complete' && orderedResults.length > 0 && (
          <div className="space-y-8">
            {/* Summary header */}
            <div className="flex items-center gap-3">
              <CheckCircle2 className="w-5 h-5 text-green" />
              <h2 className="text-base font-semibold text-text-main">
                Bulk Divergence Report
              </h2>
              <span className="text-xs text-text-muted">
                {orderedResults.length} repository{orderedResults.length !== 1 ? 'ies' : 'y'}
              </span>
            </div>

            {/* Common branch list */}
            <div className="p-4 rounded-xl bg-surface-1 border border-surface-3 space-y-3">
              <div className="flex items-center gap-2 text-xs font-semibold text-text-muted uppercase tracking-wider">
                <List className="w-3.5 h-3.5" />
                Branches analyzed
              </div>
              <div className="flex flex-wrap gap-2">
                {branches.map((b) => (
                  <span
                    key={b}
                    className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium border bg-surface-2 text-text-muted border-surface-3"
                  >
                    <GitBranch className="w-3 h-3 text-text-dim" />
                    {b}
                  </span>
                ))}
              </div>
              <p className="text-[11px] text-text-dim">
                Branch existence varies per repository and is shown in each matrix below.
              </p>
            </div>

            {/* Per-repo matrices */}
            <div className="space-y-6">
              {orderedResults.map((item) => (
                <div
                  key={item.repo_guid}
                  className="p-5 rounded-xl bg-surface-1 border border-surface-3 space-y-4"
                >
                  <div className="flex items-center gap-3">
                    <div className="w-8 h-8 rounded-lg bg-accent/10 flex items-center justify-center">
                      <GitBranch className="w-4 h-4 text-accent-light" />
                    </div>
                    <div>
                      <h3 className="text-sm font-semibold text-text-main leading-tight">
                        {item.repo_name}
                      </h3>
                      <p className="text-[11px] text-text-dim font-mono truncate max-w-md">
                        {item.repo_url}
                      </p>
                    </div>
                  </div>

                  {item.error ? (
                    <div className="p-3 rounded-lg bg-red/5 border border-red/20 text-xs text-red">
                      {item.error}
                    </div>
                  ) : item.analytics ? (
                    <DivergenceMatrix
                      analytics={item.analytics}
                      branchStatuses={item.branch_statuses}
                      onCellClick={(source, target, count) =>
                        handleCellClick(item.repo_guid, item.branch_statuses, source, target, count)
                      }
                    />
                  ) : (
                    <div className="p-3 rounded-lg bg-yellow/5 border border-yellow/20 text-xs text-yellow">
                      No analytics available for this repository.
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Empty complete state */}
        {batchStream.state.status === 'complete' && orderedResults.length === 0 && (
          <div className="p-4 rounded-xl bg-yellow/5 border border-yellow/20 space-y-2">
            <div className="flex items-center gap-2 text-sm font-semibold text-yellow">
              <AlertCircle className="w-4 h-4" />
              No results
            </div>
            <p className="text-xs text-text-muted">
              No repositories were successfully analyzed. Check that the URLs and branch names are correct.
            </p>
          </div>
        )}
      </main>

      <CommitDetailPanel
        isOpen={selectedCell !== null}
        onClose={() => setSelectedCell(null)}
        repoGuid={selectedCell?.repoGuid ?? ''}
        sourceBranch={selectedCell?.sourceBranch ?? ''}
        targetBranch={selectedCell?.targetBranch ?? ''}
        branches={selectedCell?.branches ?? []}
        totalCommits={selectedCell?.commitCount ?? 0}
        accessToken={token}
      />

      <footer className="border-t border-surface-3 mt-auto">
        <div className="max-w-6xl mx-auto px-6 py-4 flex items-center justify-between text-[11px] text-text-dim">
          <span>GitDiverge Client</span>
        </div>
      </footer>
    </div>
  )
}
