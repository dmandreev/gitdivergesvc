import { useCallback, useRef, useState } from 'react'
import type { DivergenceResponse, ProgressEventPayload } from '../api/client'
import { divergenceStream } from '../api/client'

export type StreamStatus = 'idle' | 'connecting' | 'streaming' | 'complete' | 'error'

export interface StreamState<T = DivergenceResponse> {
  status: StreamStatus
  progress?: ProgressEventPayload
  result?: T
  error?: { message: string; details?: string }
}

export function enrichProgress(event: ProgressEventPayload, phaseMessage: string): ProgressEventPayload {
  if (event.event === 'advance' && !event.message && phaseMessage) {
    return { ...event, message: phaseMessage }
  }
  return event
}

export function useDivergenceStream(accessToken: string | null) {
  const [state, setState] = useState<StreamState<DivergenceResponse>>({ status: 'idle' })
  const abortRef = useRef<AbortController | null>(null)
  const phaseMessageRef = useRef<string>('')

  const start = useCallback(
    async (repoGuid: string, branches: string[]) => {
      abortRef.current?.abort()
      const controller = new AbortController()
      abortRef.current = controller
      phaseMessageRef.current = ''
      setState({
        status: 'connecting',
        progress: { event: 'start', message: 'Connecting…', total: undefined },
      })

      try {
        for await (const payload of divergenceStream(repoGuid, branches, accessToken, controller.signal)) {
          if (controller.signal.aborted) return

          switch (payload.type) {
            case 'progress': {
              const evt = payload.event
              if (evt.event === 'start' && evt.message) {
                phaseMessageRef.current = evt.message
              }
              const enriched = enrichProgress(evt, phaseMessageRef.current)
              setState((s) => ({ ...s, status: 'streaming', progress: enriched }))
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
