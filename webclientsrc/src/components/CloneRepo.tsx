import { useState } from 'react'
import { Plus, Loader2, RefreshCw } from 'lucide-react'
import { cloneRepo } from '../api/client'
import { useAuth } from '../auth/AuthProvider'
import type { CloneRequest, CloneResponse } from '../api/client'

interface CloneRepoProps {
  onCloned: () => void
}

export function CloneRepo({ onCloned }: CloneRepoProps) {
  const { getAccessToken } = useAuth()
  const [url, setUrl] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [info, setInfo] = useState<string | null>(null)
  const [showForce, setShowForce] = useState(false)

  const runClone = async (force: boolean) => {
    if (!url.trim()) return
    setLoading(true)
    setError(null)
    setInfo(null)
    setShowForce(false)
    try {
      const { stream } = await cloneRepo({
        body: { url: url.trim(), force } satisfies CloneRequest,
        auth: getAccessToken() ?? undefined,
      })

      for await (const event of stream) {
        const payload = event as unknown as Record<string, unknown>

        if (payload && payload.type === 'error') {
          throw new Error(String(payload.message ?? 'Clone failed'))
        }

        if (payload && payload.type === 'complete') {
          const data = payload.data as CloneResponse | undefined
          if (data?.status === 'already_exists') {
            setInfo(data.message)
            setShowForce(true)
          } else {
            setUrl('')
            onCloned()
          }
          break
        }
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Clone failed')
    } finally {
      setLoading(false)
    }
  }

  const handleClone = (e: React.FormEvent) => {
    e.preventDefault()
    runClone(false)
  }

  return (
    <form onSubmit={handleClone} className="space-y-3">
      <div className="flex gap-2">
        <input
          type="text"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://github.com/owner/repo.git"
          className="flex-1 min-w-0 px-3 py-2 rounded-lg bg-surface-1 border border-surface-3 text-sm text-text-main placeholder:text-text-dim focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/30 transition-all"
        />
        <button
          type="submit"
          disabled={loading || !url.trim()}
          className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-semibold text-white bg-gradient-to-r from-accent to-accent-light hover:opacity-90 disabled:opacity-40 transition-opacity shadow-lg shadow-accent-glow"
        >
          {loading ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <Plus className="w-4 h-4" />
          )}
          Clone
        </button>
      </div>
      {error && (
        <div className="px-3 py-2 rounded-lg bg-red/10 border border-red/20 text-xs text-red">
          {error}
        </div>
      )}
      {info && (
        <div className="flex items-center justify-between gap-3 px-3 py-2 rounded-lg bg-amber/10 border border-amber/20 text-xs text-amber">
          <span>{info}</span>
          {showForce && (
            <button
              type="button"
              onClick={() => runClone(true)}
              disabled={loading}
              className="flex items-center gap-1 px-2 py-1 rounded-md bg-amber/20 hover:bg-amber/30 disabled:opacity-40 transition-colors whitespace-nowrap font-medium"
            >
              {loading ? (
                <Loader2 className="w-3 h-3 animate-spin" />
              ) : (
                <RefreshCw className="w-3 h-3" />
              )}
              Force re-clone
            </button>
          )}
        </div>
      )}
    </form>
  )
}
