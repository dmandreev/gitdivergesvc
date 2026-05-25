import type { ProgressEventPayload } from '../api/client'

interface ProgressBarProps {
  progress?: ProgressEventPayload
}

export function ProgressBar({ progress }: ProgressBarProps) {
  if (!progress) return null

  const isIndeterminate =
    progress.event !== 'finish' &&
    (progress.total == null || progress.total === 0)

  let percent = 0
  if (progress.event === 'finish') {
    percent = 100
  } else if (progress.event === 'advance' && progress.total && progress.total > 0) {
    percent = Math.min(100, Math.round((progress.current / progress.total) * 100))
  }
  return (
    <div className="w-full space-y-2">
      <div className="flex items-center justify-between text-xs">
        <span className="text-text-muted font-medium">{progress.message}</span>
        {progress.event === 'advance' && progress.total != null && (
          <span className="text-text-dim font-mono tabular-nums">
            {progress.current} / {progress.total}
          </span>
        )}
      </div>
      <div
        className="w-full h-2 rounded-full bg-surface-2 overflow-hidden relative"
        role="progressbar"
        aria-valuenow={isIndeterminate ? undefined : percent}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        {isIndeterminate ? (
          <div
            className="h-full rounded-full bg-gradient-to-r from-accent to-accent-light absolute"
            style={{
              width: '40%',
              animation: 'indeterminate-progress 1.2s infinite ease-in-out alternate',
            }}
          />
        ) : (
          <div
            className="h-full rounded-full bg-gradient-to-r from-accent to-accent-light transition-all duration-150 ease-out"
            style={{ width: `${percent}%` }}
          />
        )}
      </div>
      <style>{`
        @keyframes indeterminate-progress {
          0% { left: 0%; }
          100% { left: 60%; }
        }
      `}</style>
    </div>
  )
}
