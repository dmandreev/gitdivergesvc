import React, { useMemo, useCallback } from 'react'
import { ArrowRight, GitCommit } from 'lucide-react'
import type { BranchAnalyticsSummary, BranchStatus } from '../api/client'

interface DivergenceMatrixProps {
  analytics: BranchAnalyticsSummary
  branchStatuses: BranchStatus[]
  onCellClick: (sourceBranch: string, targetBranch: string, commitCount: number) => void
}

function commitColor(count: number): string {
  if (count === 0) {
    return 'bg-surface-1/80 text-text-dim border-surface-3/60 hover:border-surface-3'
  }
  if (count >= 1 && count <= 9) {
    return 'bg-gradient-to-br from-green/20 to-green/5 text-green border-green/30 hover:border-green/50 shadow-sm shadow-green-glow/20'
  }
  if (count >= 10 && count <= 20) {
    return 'bg-gradient-to-br from-yellow/20 to-yellow/5 text-yellow border-yellow/30 hover:border-yellow/50 shadow-sm shadow-yellow-glow/20'
  }
  return 'bg-gradient-to-br from-red/20 to-red/5 text-red border-red/30 hover:border-red/50 shadow-sm shadow-red-glow/20'
}

function countBadge(count: number): string {
  if (count === 0) return 'bg-surface-3/60 text-text-dim'
  if (count >= 1 && count <= 9) return 'bg-green/20 text-green'
  if (count >= 10 && count <= 20) return 'bg-yellow/20 text-yellow'
  return 'bg-red/20 text-red'
}

export function DivergenceMatrix({ analytics, branchStatuses, onCellClick }: DivergenceMatrixProps) {
  const branches = useMemo(
    () => analytics.branches,
    [analytics.branches]
  )

  const orderedBranchStatuses = useMemo(() => {
    return branchStatuses
  }, [branchStatuses])

  const isWideMatrix = branches.length >= 9 && branches.length <= 10
  const needsVerticalScroll = branches.length > 10
  const colMinWidth = isWideMatrix ? '7.5rem' : '5rem'
  const headerMaxWidth = isWideMatrix ? 'max-w-[10rem]' : 'max-w-[8rem]'
  const rowLabelWidth = isWideMatrix ? 'w-[8rem]' : 'w-[6rem]'

  const matrix = useMemo(() => {
    const map = new Map<string, Map<string, number>>()
    for (const comp of analytics.comparisons) {
      if (!map.has(comp.source_branch)) {
        map.set(comp.source_branch, new Map())
      }
      map.get(comp.source_branch)!.set(comp.target_branch, comp.missing_commit_count)
    }
    return map
  }, [analytics.comparisons])

  const handleOpen = useCallback(
    (source: string, target: string) => {
      const count = matrix.get(source)?.get(target) ?? 0
      if (count === 0) return
      onCellClick(source, target, count)
    },
    [matrix, onCellClick]
  )

  return (
    <div className="space-y-6">
      {/* Branch statuses */}
      <div className="flex flex-wrap gap-2">
        {orderedBranchStatuses.map((bs) => (
          <div
            key={bs.branch}
            className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium border ${
              bs.exists
                ? 'bg-green/10 text-green border-green/20'
                : 'bg-red/10 text-red border-red/20'
            }`}
          >
            <span
              className={`w-1.5 h-1.5 rounded-full ${bs.exists ? 'bg-green' : 'bg-red'}`}
            />
            {bs.branch}
          </div>
        ))}
      </div>

      {/* Matrix */}
      <div className={needsVerticalScroll ? 'max-h-[65vh] overflow-y-auto' : ''}>
        <div className="overflow-x-auto">
          <div
            className="grid"
            style={{
              gridTemplateColumns: `auto repeat(${branches.length}, minmax(${colMinWidth}, 1fr))`,
            }}
          >
            {/* Header row */}
            <div className="p-2" />
            {branches.map((b) => (
              <div
                key={`col-${b}`}
                className="px-2 py-3 text-center text-[11px] font-semibold text-text-muted uppercase tracking-wider"
              >
                <div className="flex items-center justify-center gap-1">
                  <GitCommit className="w-3 h-3" />
                  <span className={`truncate ${headerMaxWidth}`} title={b}>
                    {b}
                  </span>
                </div>
                <div className="text-[10px] text-text-dim font-normal normal-case mt-0.5">
                  target
                </div>
              </div>
            ))}

            {/* Data rows */}
            {branches.map((source) => (
              <React.Fragment key={`row-${source}`}>
                <div className="px-3 py-2 flex items-center text-[11px] font-semibold text-text-muted uppercase tracking-wider">
                  <div className="flex items-center gap-1.5">
                    <span className={`truncate ${rowLabelWidth} text-right`} title={source}>
                      {source}
                    </span>
                    <ArrowRight className="w-3 h-3 text-text-dim shrink-0" />
                  </div>
                </div>
                {branches.map((target) => {
                  const count = matrix.get(source)?.get(target) ?? 0
                  const isDiagonal = source === target
                  const clickable = !isDiagonal && count > 0

                  return (
                    <div key={`cell-${source}-${target}`} className="p-1">
                      {isDiagonal ? (
                        <div className="h-full min-h-[3rem] rounded-lg bg-surface-1/40 border border-surface-3/30 flex items-center justify-center">
                          <span className="text-text-dim text-xs">—</span>
                        </div>
                      ) : clickable ? (
                        <button
                          onClick={() => handleOpen(source, target)}
                          className={`w-full h-full min-h-[3rem] rounded-lg border px-2 py-2 flex flex-col items-center justify-center gap-0.5 transition-all ${commitColor(count)}`}
                          title={`${source} → ${target}: ${count} missing commits`}
                        >
                          <span
                            className={`text-[10px] font-bold px-1.5 py-0.5 rounded-full ${countBadge(count)}`}
                          >
                            {count}
                          </span>
                          <span className="text-[10px] opacity-70 font-medium">
                            {count === 1 ? 'commit' : 'commits'}
                          </span>
                        </button>
                      ) : (
                        <div
                          className={`h-full min-h-[3rem] rounded-lg border px-2 py-2 flex flex-col items-center justify-center gap-0.5 transition-all cursor-default ${commitColor(count)}`}
                          title={`${source} → ${target}: ${count} missing commits`}
                        >
                          <span
                            className={`text-[10px] font-bold px-1.5 py-0.5 rounded-full ${countBadge(count)}`}
                          >
                            {count}
                          </span>
                          <span className="text-[10px] opacity-70 font-medium">
                            {count === 1 ? 'commit' : 'commits'}
                          </span>
                        </div>
                      )}
                    </div>
                  )
                })}
              </React.Fragment>
            ))}
          </div>
        </div>
      </div>

      {/* Legend */}
      <div className="flex flex-wrap items-center gap-3 pt-2">
        <span className="text-[11px] text-text-dim uppercase tracking-wider font-medium">
          Legend
        </span>
        <div className="flex items-center gap-1.5">
          <span className="w-3 h-3 rounded-sm bg-surface-1 border border-surface-3" />
          <span className="text-[11px] text-text-muted">0</span>
        </div>
        <div className="flex items-center gap-1.5">
          <span className="w-3 h-3 rounded-sm bg-gradient-to-br from-green/40 to-green/10 border border-green/30" />
          <span className="text-[11px] text-text-muted">1–9</span>
        </div>
        <div className="flex items-center gap-1.5">
          <span className="w-3 h-3 rounded-sm bg-gradient-to-br from-yellow/40 to-yellow/10 border border-yellow/30" />
          <span className="text-[11px] text-text-muted">10–20</span>
        </div>
        <div className="flex items-center gap-1.5">
          <span className="w-3 h-3 rounded-sm bg-gradient-to-br from-red/40 to-red/10 border border-red/30" />
          <span className="text-[11px] text-text-muted">21+</span>
        </div>
      </div>
    </div>
  )
}
