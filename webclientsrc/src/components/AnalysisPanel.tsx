import { useState } from 'react'
import { Play, Square, AlertCircle, GitCommit, CheckCircle2, RefreshCw } from 'lucide-react'
import type { RepoIndexEntry } from '../api/client'
import type { BranchStatus } from '../generated/types.gen'
import { useAuth } from '../auth/AuthProvider'
import { CONFIG } from '../config'
import { useDivergenceStream } from '../hooks/useDivergenceStream'
import { useFetchStream } from '../hooks/useFetchStream'
import { ProgressBar } from './ProgressBar'
import { DivergenceMatrix } from './DivergenceMatrix'
import { CommitDetailPanel } from './CommitDetailPanel'
import { BranchInput } from './BranchInput'

const STORAGE_KEY = 'gitdiverge_branches'
const DEFAULT_BRANCHES = 'main, develop, stage'

interface AnalysisPanelProps {
  repo: RepoIndexEntry
}

export function AnalysisPanel({ repo }: AnalysisPanelProps) {
  const { getAccessToken } = useAuth()
  const token = getAccessToken()
  const divergence = useDivergenceStream(token)
  const fetchStream = useFetchStream(token)
  const [branchesInput, setBranchesInput] = useState(() => {
    try {
      return localStorage.getItem(STORAGE_KEY) ?? DEFAULT_BRANCHES
    } catch {
      return DEFAULT_BRANCHES
    }
  })
  const [selectedCell, setSelectedCell] = useState<{
    sourceBranch: string
    targetBranch: string
    commitCount: number
  } | null>(null)


  const branches = branchesInput
    .split(',')
    .map((b) => b.trim())
    .filter(Boolean)

  const isBusy =
    divergence.state.status === 'connecting' ||
    divergence.state.status === 'streaming' ||
    fetchStream.state.status === 'connecting' ||
    fetchStream.state.status === 'streaming'

  const handleAnalyze = () => {
    if (branches.length === 0) return
    divergence.start(repo.guid, branches)
  }

  const handleRefresh = () => {
    if (branches.length === 0) return
    fetchStream.start(repo.guid, branches)
  }

  const handleInputChange = (value: string) => {
    setBranchesInput(value)
    try {
      localStorage.setItem(STORAGE_KEY, value)
    } catch {
      // ignore storage errors
    }
  }

  const needsToken = CONFIG.USE_AUTH && !token

  const activeProgress =
    fetchStream.state.status === 'connecting' || fetchStream.state.status === 'streaming'
      ? fetchStream.state.progress
      : divergence.state.status === 'connecting' || divergence.state.status === 'streaming'
        ? divergence.state.progress
        : undefined

  const activeLabel =
    fetchStream.state.status === 'connecting' || fetchStream.state.status === 'streaming'
      ? 'Refreshing repository…'
      : divergence.state.status === 'connecting' || divergence.state.status === 'streaming'
        ? 'Running divergence analysis…'
        : ''

  return (
    <div className="space-y-6">
      <div className="space-y-2">
        <label className="text-xs font-semibold text-text-muted uppercase tracking-wider">
          Branches to compare
        </label>
        <div className="flex flex-col sm:flex-row gap-4">
          <div className="flex-1">
            <BranchInput
              value={branchesInput}
              onChange={handleInputChange}
              onSubmit={handleAnalyze}
              repoGuid={repo.guid}
              token={token}
              disabled={needsToken}
            />
          </div>
          <div className="flex items-center justify-center sm:justify-start gap-2">
            {isBusy ? (
              <button
                onClick={() => {
                  divergence.cancel()
                  fetchStream.cancel()
                }}
                className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-semibold text-red bg-red/10 hover:bg-red/20 border border-red/20 transition-colors"
              >
                <Square className="w-4 h-4 fill-current" />
                Cancel
              </button>
            ) : (
              <>
                <button
                  onClick={handleRefresh}
                  disabled={needsToken || branches.length === 0}
                  className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-semibold text-text-main bg-surface-2 hover:bg-surface-3 border border-surface-3 disabled:opacity-40 transition-colors"
                >
                  <RefreshCw className="w-4 h-4" />
                  Refresh
                </button>
                <button
                  onClick={handleAnalyze}
                  disabled={needsToken || branches.length === 0}
                  className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-semibold text-white bg-gradient-to-r from-accent to-accent-light hover:opacity-90 disabled:opacity-40 transition-opacity shadow-lg shadow-accent-glow"
                >
                  <Play className="w-4 h-4 fill-current" />
                  Analyse
                </button>
              </>
            )}
          </div>
        </div>
        <p className="text-[11px] text-text-dim">
          Comma-separated branch names. The analysis generates n×(n−1) pairwise comparisons.
        </p>
      </div>

      {/* Progress */}
      {activeProgress && (
        <div className="p-4 rounded-xl bg-surface-1 border border-surface-3 space-y-3">
          <div className="flex items-center gap-2 text-xs text-text-muted font-medium">
            <GitCommit className="w-3.5 h-3.5 text-accent-light" />
            {activeLabel}
          </div>
          <ProgressBar progress={activeProgress} />
        </div>
      )}

      {/* Fetch Error */}
      {fetchStream.state.status === 'error' && fetchStream.state.error && (
        <div className="p-4 rounded-xl bg-red/5 border border-red/20 space-y-2">
          <div className="flex items-center gap-2 text-sm font-semibold text-red">
            <AlertCircle className="w-4 h-4" />
            Refresh failed
          </div>
          <p className="text-xs text-text-muted">{fetchStream.state.error.message}</p>
          {fetchStream.state.error.details && (
            <pre className="text-[11px] text-text-dim bg-surface-0 p-2 rounded-lg overflow-x-auto">
              {fetchStream.state.error.details}
            </pre>
          )}
        </div>
      )}

      {/* Fetch Success */}
      {fetchStream.state.status === 'complete' && fetchStream.state.result && (
        <div className="flex items-center gap-2 p-3 rounded-xl bg-green/5 border border-green/20">
          <CheckCircle2 className="w-4 h-4 text-green" />
          <span className="text-xs text-green">
            {fetchStream.state.result.message}
          </span>
        </div>
      )}

      {/* Analysis Error */}
      {divergence.state.status === 'error' && divergence.state.error && (
        <div className="p-4 rounded-xl bg-red/5 border border-red/20 space-y-2">
          <div className="flex items-center gap-2 text-sm font-semibold text-red">
            <AlertCircle className="w-4 h-4" />
            Analysis failed
          </div>
          <p className="text-xs text-text-muted">{divergence.state.error.message}</p>
          {divergence.state.error.details && (
            <pre className="text-[11px] text-text-dim bg-surface-0 p-2 rounded-lg overflow-x-auto">
              {divergence.state.error.details}
            </pre>
          )}
        </div>
      )}

      {/* Analysis Result */}
      {divergence.state.status === 'complete' && divergence.state.result && (
        <div className="space-y-4">
          <div className="flex items-center gap-2">
            <CheckCircle2 className="w-5 h-5 text-green" />
            <h3 className="text-sm font-semibold text-text-main">
              {divergence.state.result.repo_name}
            </h3>
          </div>
          <div className="p-4 rounded-xl bg-surface-1 border border-surface-3">
            <DivergenceMatrix
              analytics={divergence.state.result.analytics}
              branchStatuses={divergence.state.result.branch_statuses}
              onCellClick={(source, target, count) =>
                setSelectedCell({ sourceBranch: source, targetBranch: target, commitCount: count })
              }
            />
          </div>
        </div>
      )}

      <CommitDetailPanel
        isOpen={selectedCell !== null}
        onClose={() => setSelectedCell(null)}
        repoGuid={repo.guid}
        sourceBranch={selectedCell?.sourceBranch ?? ''}
        targetBranch={selectedCell?.targetBranch ?? ''}
        branches={
          divergence.state.result?.branch_statuses
            ?.filter((bs: BranchStatus) => bs.exists)
            .map((bs: BranchStatus) => bs.branch) ?? []
        }
        totalCommits={selectedCell?.commitCount ?? 0}
      />
    </div>
  )
}
