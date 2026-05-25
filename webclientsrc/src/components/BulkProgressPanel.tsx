import { CheckCircle2, GitBranch, Download, BarChart3 } from 'lucide-react'
import type { ProgressEventPayload } from '../api/client'

interface BulkProgressPanelProps {
  progress?: ProgressEventPayload
  phase?: number
  phaseComplete?: boolean
  activeRepo?: string
}

const PHASES = [
  { icon: GitBranch, label: 'Clone repositories', verb: 'cloned' },
  { icon: Download, label: 'Fetch updates & validate branches', verb: 'fetched' },
  { icon: BarChart3, label: 'Analyze branch divergence', verb: 'analyzed' },
]

function getPhaseStatus(
  index: number,
  phase: number,
  phaseComplete: boolean
): 'pending' | 'active' | 'complete' {
  if (phase === 0) return index === 0 ? 'active' : 'pending'
  if (phase > index + 1) return 'complete'
  if (phase < index + 1) return 'pending'
  return phaseComplete ? 'complete' : 'active'
}

function formatCount(progress: ProgressEventPayload): string {
  if (progress.event === 'finish') return 'Done'
  const total = progress.total
  if (total == null || total === 0) return 'Working…'
  const current = progress.event === 'advance' ? progress.current : 0
  // Backend counts 2 progress units per repository (start + finish),
  // so divide by 2 to show the actual repository count.
  return `${Math.ceil(current / 2)} / ${total / 2}`
}

function percent(progress: ProgressEventPayload): number {
  if (progress.event === 'finish') return 100
  const total = progress.total
  if (total == null || total === 0) return 0
  const current = progress.event === 'advance' ? progress.current : 0
  return Math.min(100, Math.round((current / total) * 100))
}

export function BulkProgressPanel({ progress, phase, phaseComplete, activeRepo }: BulkProgressPanelProps) {
  return (
    <div className="space-y-3">
      {PHASES.map((p, idx) => {
        const status = getPhaseStatus(idx, phase ?? 0, phaseComplete ?? false)
        const isActive = status === 'active'
        const Icon = p.icon
        const count = isActive && progress ? formatCount(progress) : undefined
        const pct = isActive && progress ? percent(progress) : status === 'complete' ? 100 : 0

        return (
          <div
            key={p.label}
            className={`flex items-center gap-4 p-3 rounded-xl border transition-colors ${
              isActive
                ? 'bg-accent/5 border-accent/20'
                : status === 'complete'
                  ? 'bg-green/5 border-green/20'
                  : 'bg-surface-1 border-surface-3'
            }`}
          >
            {/* Icon / status indicator */}
            <div className="shrink-0">
              {status === 'complete' ? (
                <div className="w-10 h-10 rounded-full bg-green/20 flex items-center justify-center">
                  <CheckCircle2 className="w-5 h-5 text-green" />
                </div>
              ) : (
                <div
                  className={`w-10 h-10 rounded-full flex items-center justify-center ${
                    isActive ? 'bg-accent/20' : 'bg-surface-2'
                  }`}
                >
                  <Icon
                    className={`w-5 h-5 ${
                      isActive ? 'text-accent-light' : 'text-text-dim'
                    }`}
                  />
                </div>
              )}
            </div>

            {/* Info */}
            <div className="flex-1 min-w-0">
              <div className="flex items-center justify-between gap-3">
                <span
                  className={`text-sm font-semibold ${
                    isActive
                      ? 'text-text-main'
                      : status === 'complete'
                        ? 'text-green'
                        : 'text-text-dim'
                  }`}
                >
                  {p.label}
                </span>

                {count && (
                  <span className="text-lg font-bold tabular-nums text-accent-light shrink-0">
                    {count}
                  </span>
                )}

                {status === 'complete' && !count && (
                  <span className="text-sm font-semibold text-green shrink-0">Done</span>
                )}

                {status === 'pending' && !count && (
                  <span className="text-sm text-text-dim shrink-0">Waiting…</span>
                )}
              </div>

              {/* Progress bar */}
              <div className="mt-2 w-full h-2 rounded-full bg-surface-2 overflow-hidden relative">
                <div
                  className={`h-full rounded-full transition-all duration-300 ease-out ${
                    status === 'complete'
                      ? 'bg-green'
                      : isActive
                        ? 'bg-gradient-to-r from-accent to-accent-light'
                        : 'bg-transparent'
                  }`}
                  style={{ width: `${Math.max(2, pct)}%` }}
                />
              </div>

              {/* Percentage text */}
              {isActive && (
                <div className="mt-1 text-[11px] text-text-muted tabular-nums">
                  {pct}%
                </div>
              )}

              {/* Active repository */}
              {isActive && activeRepo && (
                <div className="mt-1 text-xs text-text-muted truncate" title={activeRepo}>
                  {activeRepo}
                </div>
              )}
            </div>
          </div>
        )
      })}
    </div>
  )
}
