import { useCallback, useRef, useState } from 'react'
import type { BatchDivergenceResponseItem, ProgressEventPayload } from '../api/client'
import { batchDivergenceStream } from '../api/client'

export type StreamStatus = 'idle' | 'connecting' | 'streaming' | 'complete' | 'error'

export interface StreamState<T = BatchDivergenceResponseItem[]> {
  status: StreamStatus
  progress?: ProgressEventPayload
  phase?: number
  phaseComplete?: boolean
  result?: T
  error?: { message: string; details?: string }
  activeRepo?: string
}

export function enrichProgress(event: ProgressEventPayload, phaseMessage: string): ProgressEventPayload {
  if (event.event === 'advance' && !event.message && phaseMessage) {
    return { ...event, message: phaseMessage }
  }
  return event
}

function parsePhase(message: string): number {
  const match = message.match(/phase (\d)\/3/)
  return match ? parseInt(match[1], 10) : 0
}

export function useBatchDivergenceStream(accessToken: string | null) {
  const [state, setState] = useState<StreamState<BatchDivergenceResponseItem[]>>({ status: 'idle' })
  const abortRef = useRef<AbortController | null>(null)
  const phaseMessageRef = useRef<string>('')

  const start = useCallback(
    async (repos: { url: string }[], branches: string[]) => {
      abortRef.current?.abort()
      const controller = new AbortController()
      abortRef.current = controller
      phaseMessageRef.current = ''
      setState({
        status: 'connecting',
        progress: { event: 'start', message: 'Connecting…', total: undefined },
      })

      try {
        for await (const payload of batchDivergenceStream(repos, branches, accessToken, controller.signal)) {
          if (controller.signal.aborted) return

          switch (payload.type) {
            case 'progress': {
              const evt = payload.event
              let phase = 0
              if (evt.event === 'start' && evt.message) {
                phaseMessageRef.current = evt.message
                phase = parsePhase(evt.message)
              } else {
                phase = parsePhase(phaseMessageRef.current)
              }
              const enriched = enrichProgress(evt, phaseMessageRef.current)
              const phaseComplete = evt.event === 'finish'
              const rawMessage = evt.event === 'advance' ? evt.message : ''
              const isRepoMessage = rawMessage && rawMessage !== phaseMessageRef.current
              setState((s) => ({
                ...s,
                status: 'streaming',
                progress: enriched,
                phase,
                phaseComplete,
                activeRepo: evt.event === 'start' ? undefined : (isRepoMessage ? rawMessage : s.activeRepo),
              }))
              break
            }
            case 'complete':
              setState({ status: 'complete', result: payload.data })
              return
            case 'error':
              setState({ status: 'error', error: { message: payload.message, details: payload.details } })
              return
          }
        }
      } catch (err) {
        if (controller.signal.aborted) return
        const message = err instanceof Error ? err.message : String(err)
        setState({ status: 'error', error: { message } })
      }
    },
    [accessToken]
  )

  const cancel = useCallback(() => {
    abortRef.current?.abort()
    setState({ status: 'idle' })
  }, [])

  return { state, start, cancel }
}
