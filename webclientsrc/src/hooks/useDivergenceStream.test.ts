import { renderHook, act, waitFor } from '@testing-library/react'
import { useDivergenceStream, enrichProgress } from './useDivergenceStream'
import * as client from '../api/client'
import type { DivergenceResponse, SsePayload } from '../api/client'

function makeAsyncGenerator(payloads: SsePayload[]) {
  return async function* () {
    for (const p of payloads) yield p
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }() as any
}

const mockResult: DivergenceResponse = {
  repo_name: 'r',
  repo_guid: 'g',
  repo_path: '/p',
  branch_statuses: [],
  analytics: {
    repo_path: '/p',
    branches: ['main'],
    comparisons: [],
  },
}

describe('useDivergenceStream', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('starts idle', () => {
    const { result } = renderHook(() => useDivergenceStream('token'))
    expect(result.current.state.status).toBe('idle')
  })

  it('transitions through states on success', async () => {
    vi.spyOn(client, 'divergenceStream').mockReturnValue(
      makeAsyncGenerator([
        { type: 'progress', event: { event: 'start', total: 2, message: 'start' } },
        { type: 'progress', event: { event: 'advance', current: 1, total: 2, message: 'mid' } },
        { type: 'complete', data: mockResult },
      ])
    )

    const { result } = renderHook(() => useDivergenceStream('token'))

    act(() => {
      result.current.start('guid', ['main'])
    })

    await waitFor(() => expect(result.current.state.status).toBe('complete'))
    expect(result.current.state.result).toEqual(mockResult)
  })

  it('enriches empty advance messages with the last start message', () => {
    const startEvent: client.ProgressEventPayload = { event: 'start', total: 16, message: 'analyzing branches' }
    const advanceEvent: client.ProgressEventPayload = { event: 'advance', current: 1, total: 16, message: '' }

    expect(enrichProgress(advanceEvent, 'analyzing branches')).toEqual({
      event: 'advance',
      current: 1,
      total: 16,
      message: 'analyzing branches',
    })

    // Non-empty messages are preserved
    expect(enrichProgress({ ...advanceEvent, message: 'mid' }, 'analyzing branches')).toEqual({
      event: 'advance',
      current: 1,
      total: 16,
      message: 'mid',
    })

    // Start events are untouched
    expect(enrichProgress(startEvent, '')).toEqual(startEvent)
  })

  it('handles error events', async () => {
    vi.spyOn(client, 'divergenceStream').mockReturnValue(
      makeAsyncGenerator([
        { type: 'progress', event: { event: 'start', message: 'start' } },
        { type: 'error', message: 'fail', details: 'detail' },
      ])
    )

    const { result } = renderHook(() => useDivergenceStream('token'))

    act(() => {
      result.current.start('guid', ['main'])
    })

    await waitFor(() => expect(result.current.state.status).toBe('error'))
    expect(result.current.state.error).toEqual({ message: 'fail', details: 'detail' })
  })

  it('cancels and resets to idle', async () => {
    vi.spyOn(client, 'divergenceStream').mockReturnValue(
      makeAsyncGenerator([
        { type: 'progress', event: { event: 'start', message: 'start' } },
      ])
    )

    const { result } = renderHook(() => useDivergenceStream('token'))

    act(() => {
      result.current.start('guid', ['main'])
    })

    await waitFor(() => expect(result.current.state.status).toBe('streaming'))

    act(() => {
      result.current.cancel()
    })

    expect(result.current.state.status).toBe('idle')
  })
})
