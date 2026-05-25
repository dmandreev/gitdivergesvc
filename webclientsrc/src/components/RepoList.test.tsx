import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { RepoList } from './RepoList'
import { AuthProvider } from '../auth/AuthProvider'

vi.hoisted(() => {
  window.__GITDIVERGE_CONFIG__ = { USE_AUTH: false }
})

const mockRepos = [
  { guid: 'g1', name: 'alpha-repo', repo_path: '/tmp/alpha', url: 'https://github.com/user/alpha-repo' },
  { guid: 'g2', name: 'beta-XY-project', repo_path: '/tmp/beta', url: 'https://gitlab.com/org/beta' },
  { guid: 'g3', name: 'gamma', repo_path: '/tmp/gamma', url: 'https://github.com/user/nmXYnm' },
  { guid: 'g4', name: 'zzzzXY', repo_path: '/tmp/xy', url: 'https://github.com/user/zzzzXY' },
]

function mockFetchResponse(data: unknown) {
  globalThis.fetch = vi.fn().mockResolvedValue({
    ok: true,
    status: 200,
    headers: new Headers({ 'content-type': 'application/json' }),
    json: () => Promise.resolve(data),
    text: () => Promise.resolve(JSON.stringify(data)),
  } as Response)
}

describe('RepoList', () => {
  let originalGetBoundingClientRect: typeof HTMLElement.prototype.getBoundingClientRect
  let originalResizeObserver: typeof ResizeObserver

  beforeAll(() => {
    originalGetBoundingClientRect = HTMLElement.prototype.getBoundingClientRect
    HTMLElement.prototype.getBoundingClientRect = function () {
      return {
        width: 400,
        height: 400,
        top: 0,
        left: 0,
        bottom: 400,
        right: 400,
        x: 0,
        y: 0,
        toJSON: () => {},
      } as DOMRect
    }

    originalResizeObserver = window.ResizeObserver
    window.ResizeObserver = class MockResizeObserver {
      callback: ResizeObserverCallback
      constructor(callback: ResizeObserverCallback) {
        this.callback = callback
      }
      observe(target: Element) {
        this.callback(
          [
            {
              target,
              contentRect: { width: 400, height: 400 } as DOMRectReadOnly,
              borderBoxSize: [{ inlineSize: 400, blockSize: 400 }],
              contentBoxSize: [{ inlineSize: 400, blockSize: 400 }],
              devicePixelContentBoxSize: [{ inlineSize: 400, blockSize: 400 }],
            } as ResizeObserverEntry,
          ],
          this
        )
      }
      unobserve() {}
      disconnect() {}
    }
  })

  afterAll(() => {
    HTMLElement.prototype.getBoundingClientRect = originalGetBoundingClientRect
    window.ResizeObserver = originalResizeObserver
  })

  beforeEach(() => {
    vi.clearAllMocks()
  })

  async function renderRepoList(props: Partial<React.ComponentProps<typeof RepoList>> = {}) {
    mockFetchResponse(mockRepos)
    const onSelect = vi.fn()
    render(
      <AuthProvider>
        <RepoList onSelect={onSelect} {...props} />
      </AuthProvider>
    )
    await screen.findByText('alpha-repo')
    return { onSelect }
  }

  it('renders repositories after loading', async () => {
    await renderRepoList()
    expect(screen.getByText('alpha-repo')).toBeInTheDocument()
    expect(screen.getByText('beta-XY-project')).toBeInTheDocument()
    expect(screen.getByText('gamma')).toBeInTheDocument()
    expect(screen.getByText('zzzzXY')).toBeInTheDocument()
  })

  it('filters repositories by name substring (case-insensitive)', async () => {
    const user = userEvent.setup()
    await renderRepoList()

    const input = screen.getByPlaceholderText('Filter repositories...')
    await user.type(input, 'XY')

    expect(screen.queryByText('alpha-repo')).not.toBeInTheDocument()
    expect(screen.getByText('beta-XY-project')).toBeInTheDocument()
    expect(screen.getByText('gamma')).toBeInTheDocument()
    expect(screen.getByText('zzzzXY')).toBeInTheDocument()
  })

  it('filters repositories by URL substring (case-insensitive)', async () => {
    const user = userEvent.setup()
    await renderRepoList()

    const input = screen.getByPlaceholderText('Filter repositories...')
    await user.type(input, 'nmXYnm')

    expect(screen.queryByText('alpha-repo')).not.toBeInTheDocument()
    expect(screen.queryByText('beta-XY-project')).not.toBeInTheDocument()
    expect(screen.getByText('gamma')).toBeInTheDocument()
    expect(screen.queryByText('zzzzXY')).not.toBeInTheDocument()
  })

  it('shows empty state when filter matches nothing', async () => {
    const user = userEvent.setup()
    await renderRepoList()

    const input = screen.getByPlaceholderText('Filter repositories...')
    await user.type(input, 'nonexistent')

    expect(screen.queryByText('alpha-repo')).not.toBeInTheDocument()
    expect(screen.getByText('No repositories match your filter.')).toBeInTheDocument()
  })

  it('clears filter when clicking the clear button', async () => {
    const user = userEvent.setup()
    await renderRepoList()

    const input = screen.getByPlaceholderText('Filter repositories...')
    await user.type(input, 'alpha')
    expect(screen.queryByText('beta-XY-project')).not.toBeInTheDocument()

    await user.click(screen.getByLabelText('Clear filter'))
    expect(screen.getByText('beta-XY-project')).toBeInTheDocument()
  })

  it('calls onSelect when a repository is clicked', async () => {
    const user = userEvent.setup()
    const { onSelect } = await renderRepoList()

    await user.click(screen.getByText('beta-XY-project'))
    expect(onSelect).toHaveBeenCalledWith(mockRepos[1])
  })

  it('highlights the selected repository', async () => {
    await renderRepoList({ selectedGuid: 'g2' })

    const selected = screen.getByText('beta-XY-project').closest('button')
    expect(selected).toHaveClass('border-accent/40')
  })

  it('reloads when refresh button is clicked', async () => {
    const user = userEvent.setup()
    await renderRepoList()

    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      headers: new Headers({ 'content-type': 'application/json' }),
      json: () => Promise.resolve(mockRepos),
      text: () => Promise.resolve(JSON.stringify(mockRepos)),
    } as Response)

    await user.click(screen.getByLabelText('Refresh repositories'))
    await waitFor(() => expect(globalThis.fetch).toHaveBeenCalledTimes(1))
  })

  it('reloads when refreshSignal prop changes', async () => {
    mockFetchResponse(mockRepos)
    const { rerender } = render(
      <AuthProvider>
        <RepoList onSelect={vi.fn()} refreshSignal={0} />
      </AuthProvider>
    )
    await screen.findByText('alpha-repo')

    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      headers: new Headers({ 'content-type': 'application/json' }),
      json: () => Promise.resolve(mockRepos),
      text: () => Promise.resolve(JSON.stringify(mockRepos)),
    } as Response)

    rerender(
      <AuthProvider>
        <RepoList onSelect={vi.fn()} refreshSignal={1} />
      </AuthProvider>
    )
    await waitFor(() => expect(globalThis.fetch).toHaveBeenCalledTimes(1))
  })

  it('displays error message when loading fails', async () => {
    globalThis.fetch = vi.fn().mockRejectedValue(new Error('Network error'))
    render(
      <AuthProvider>
        <RepoList onSelect={vi.fn()} />
      </AuthProvider>
    )

    expect(await screen.findByText('Network error')).toBeInTheDocument()
  })

  it('shows empty state when no repositories exist', async () => {
    mockFetchResponse([])
    render(
      <AuthProvider>
        <RepoList onSelect={vi.fn()} />
      </AuthProvider>
    )

    expect(await screen.findByText('No repositories indexed yet.')).toBeInTheDocument()
  })
})
