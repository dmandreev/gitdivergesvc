import { client } from '../generated/client.gen'
import {
  listRepos,
  cloneRepo,
  fetchRepo,
  deleteRepo,
  testToken,
  health,
  repoDivergence,
  listBranches,
  getDivergenceCommits,
  batchDivergence,
} from '../generated/sdk.gen'
import type {
  DivergenceResponse,
  FetchResponse,
  ProgressEvent,
  RepoIndexEntry,
  CloneRequest,
  CloneResponse,
  FetchRequest,
  TestTokenResponse,
  HealthResponse,
  Commit,
  BranchesResponse,
  PagedCommitsResponse,
  PagedCommitsRequest,
  BranchAnalyticsSummary,
  BranchComparisonSummary,
  BranchStatus,
  BatchDivergenceResponseItem,
  BatchDivergenceRequest,
  BatchRepoRequest,
} from '../generated/types.gen'
import { CONFIG } from '../config'

const API_BASE = CONFIG.API_BASE

client.setConfig({
  baseUrl: API_BASE,
})

export type {
  DivergenceResponse,
  FetchResponse,
  ProgressEvent,
  RepoIndexEntry,
  CloneRequest,
  CloneResponse,
  FetchRequest,
  TestTokenResponse,
  HealthResponse,
  Commit,
  BranchesResponse,
  PagedCommitsResponse,
  PagedCommitsRequest,
  BranchAnalyticsSummary,
  BranchComparisonSummary,
  BranchStatus,
  BatchDivergenceResponseItem,
  BatchDivergenceRequest,
  BatchRepoRequest,
}

export { listRepos, cloneRepo, fetchRepo, deleteRepo, testToken, health, repoDivergence, listBranches, getDivergenceCommits, batchDivergence }

export type ProgressEventPayload =
  | { event: 'start'; total?: number; message: string }
  | { event: 'advance'; current: number; total?: number; message: string }
  | { event: 'finish'; message: string }

export type SsePayload<T = DivergenceResponse> =
  | { type: 'progress'; event: ProgressEventPayload; ts?: number }
  | { type: 'complete'; data: T }
  | { type: 'error'; message: string; details?: string }

export async function* divergenceStream(
  repoGuid: string,
  branches: string[],
  token: string | null,
  signal?: AbortSignal
): AsyncGenerator<SsePayload<DivergenceResponse>, void, unknown> {
  const { stream } = await repoDivergence({
    path: {
      repo_guid: repoGuid,
    },
    query: {
      branches: branches.join(','),
    },
    auth: token ?? undefined,
    signal,
  })

  for await (const event of stream) {
    const payload = event as unknown as SsePayload<DivergenceResponse>
    yield payload
    if (payload.type === 'complete' || payload.type === 'error') {
      return
    }
  }
}

export async function* fetchStream(
  repoGuid: string,
  branches: string[],
  token: string | null,
  signal?: AbortSignal
): AsyncGenerator<SsePayload<FetchResponse>, void, unknown> {
  const { stream } = await fetchRepo({
    path: {
      repo_guid: repoGuid,
    },
    body: {
      branches,
    },
    auth: token ?? undefined,
    signal,
  })

  for await (const event of stream) {
    const payload = event as unknown as SsePayload<FetchResponse>
    yield payload
    if (payload.type === 'complete' || payload.type === 'error') {
      return
    }
  }
}

export async function* batchDivergenceStream(
  repos: BatchRepoRequest[],
  branches: string[],
  token: string | null,
  signal?: AbortSignal
): AsyncGenerator<SsePayload<BatchDivergenceResponseItem[]>, void, unknown> {
  const { stream } = await batchDivergence({
    body: {
      repos,
      branches,
    },
    auth: token ?? undefined,
    signal,
  })

  for await (const event of stream) {
    const payload = event as unknown as SsePayload<BatchDivergenceResponseItem[]>
    yield payload
    if (payload.type === 'complete' || payload.type === 'error') {
      return
    }
  }
}
