import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { CommitDetailPanel } from './CommitDetailPanel'
import * as client from '../api/client'
import type { Commit } from '../generated/types.gen'

function makeCommit(overrides: Partial<Commit> & { hash: string; timestamp: string; subject: string }): Commit {
  return {
    author: { name: 'Alice', email: 'alice@example.com' },
    body: null,
    parents: [],
    ...overrides,
  }
}

const page1Commits: Commit[] = [
  makeCommit({ hash: 'ccc', timestamp: '2024-03-15T10:00:00Z', subject: 'Third commit', body: 'Detailed body for third' }),
  makeCommit({ hash: 'bbb', timestamp: '2024-02-20T14:30:00Z', subject: 'Second commit', parents: ['parent1', 'parent2'] }),
]

const page2Commits: Commit[] = [
  makeCommit({ hash: 'aaa', timestamp: '2024-01-10T08:00:00Z', subject: 'First commit' }),
]

function mockGetDivergenceCommits() {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return vi.spyOn(client, 'getDivergenceCommits') as any
}

function mockResponse(commits: Commit[], page = 0, pageSize = 50) {
  return {
    data: {
      commits,
      page,
      page_size: pageSize,
      source_branch: 'main',
      target_branch: 'develop',
      total_commits: commits.length,
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
} as any
}

describe('CommitDetailPanel', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('renders nothing when closed', () => {
    render(
      <CommitDetailPanel
        isOpen={false}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('fetches commits when opened', async () => {
    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    await waitFor(() => expect(screen.getByText('Third commit')).toBeInTheDocument())
    expect(screen.getByText('Second commit')).toBeInTheDocument()
    expect(client.getDivergenceCommits).toHaveBeenCalledWith({
      path: { repo_guid: 'repo-1' },
      query: {
        branches: 'main,develop',
        source_branch: 'main',
        target_branch: 'develop',
        page: 0,
        page_size: 50,
      },
    })
  })

  it('displays loading state initially', () => {
    mockGetDivergenceCommits().mockImplementation(() => new Promise(() => {}))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    expect(screen.getByText('Loading commits…')).toBeInTheDocument()
  })

  it('renders header with branch names and commit count', async () => {
    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    const dialog = await waitFor(() => screen.getByRole('dialog', { name: 'Missing commits' }))
    expect(dialog).toBeInTheDocument()
    expect(screen.getByText('Missing commits')).toBeInTheDocument()
    expect(dialog.textContent).toContain('main')
    expect(dialog.textContent).toContain('develop')
    expect(dialog.textContent).toContain('2 commits')
  })

  it('sorts commits by date descending (newest first)', async () => {
    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    await waitFor(() => expect(screen.getByText('Third commit')).toBeInTheDocument())
    const rows = screen.getAllByRole('button').filter((b) => b.className.includes('w-full'))
    expect(rows[0]).toHaveTextContent('Third commit')
    expect(rows[1]).toHaveTextContent('Second commit')
  })

  it('filters commits by query', async () => {
    const user = userEvent.setup()
    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    await waitFor(() => expect(screen.getByText('Third commit')).toBeInTheDocument())
    const input = screen.getByPlaceholderText('Filter by subject, author, hash…')
    await user.type(input, 'Second')

    expect(screen.getByText('Second commit')).toBeInTheDocument()
    expect(screen.queryByText('Third commit')).not.toBeInTheDocument()
  })

  it('shows empty state when filter matches nothing', async () => {
    const user = userEvent.setup()
    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    await waitFor(() => expect(screen.getByText('Third commit')).toBeInTheDocument())
    const input = screen.getByPlaceholderText('Filter by subject, author, hash…')
    await user.type(input, 'zzzz')

    expect(screen.getByText('No commits match your filter.')).toBeInTheDocument()
  })

  it('shows commit detail modal when a commit is selected', async () => {
    const user = userEvent.setup()
    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    await waitFor(() => expect(screen.getByText('Third commit')).toBeInTheDocument())
    const row = screen.getByText('Third commit').closest('button')
    expect(row).toBeTruthy()
    await user.click(row!)

    const modal = screen.getByRole('dialog', { name: 'Commit details' })
    expect(modal).toBeInTheDocument()
    expect(screen.getByText('Detailed body for third')).toBeInTheDocument()
    expect(modal.textContent).toContain('Alice')
  })

  it('shows parents when a commit has them', async () => {
    const user = userEvent.setup()
    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    await waitFor(() => expect(screen.getByText('Second commit')).toBeInTheDocument())
    const row = screen.getByText('Second commit').closest('button')
    await user.click(row!)

    const modal = screen.getByRole('dialog', { name: 'Commit details' })
    expect(modal).toBeInTheDocument()
    expect(modal.textContent).toContain('Parents:')
    expect(modal.textContent).toContain('parent1')
    expect(modal.textContent).toContain('parent2')
  })

  it('calls onClose when close button is clicked', async () => {
    const user = userEvent.setup()
    const onClose = vi.fn()
    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={onClose}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    await waitFor(() => expect(screen.getByText('Third commit')).toBeInTheDocument())
    await user.click(screen.getByLabelText('Close panel'))
    expect(onClose).toHaveBeenCalled()
  })

  it('renders a resize handle', async () => {
    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    await waitFor(() => expect(screen.getByRole('separator', { name: 'Resize panel' })).toBeInTheDocument())
  })

  it('restores panel width from localStorage', async () => {
    localStorage.setItem('gitdiverge-panel-width', '500')
    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    const panel = await waitFor(() => screen.getByRole('dialog', { name: 'Missing commits' }))
    expect((panel as HTMLElement).style.width).toBe('500px')
  })

  it('persists panel width after resize', async () => {
    const originalInnerWidth = window.innerWidth
    Object.defineProperty(window, 'innerWidth', {
      writable: true,
      configurable: true,
      value: 1200,
    })

    mockGetDivergenceCommits().mockResolvedValue(mockResponse(page1Commits))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    const handle = await waitFor(() => screen.getByRole('separator', { name: 'Resize panel' }))

    fireEvent.mouseDown(handle)
    fireEvent.mouseMove(document, { clientX: 400 })
    fireEvent.mouseUp(document)

    expect(localStorage.getItem('gitdiverge-panel-width')).toBe('800')

    Object.defineProperty(window, 'innerWidth', {
      writable: true,
      configurable: true,
      value: originalInnerWidth,
    })
  })

  it('loads more commits when clicking load more', async () => {
    const user = userEvent.setup()
    const fullPage = Array.from({ length: 50 }, (_, i) =>
      makeCommit({ hash: `page1-${i}`, timestamp: '2024-03-15T10:00:00Z', subject: `Page1 commit ${i}` })
    )
    mockGetDivergenceCommits()
      .mockResolvedValueOnce(mockResponse(fullPage, 0, 50))
      .mockResolvedValueOnce(mockResponse(page2Commits, 1, 50))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={3}
      />
    )

    await waitFor(() => expect(screen.getByText('Page1 commit 0')).toBeInTheDocument())
    expect(screen.getByText('Load more commits')).toBeInTheDocument()

    await user.click(screen.getByText('Load more commits'))

    await waitFor(() => expect(screen.getByText('First commit')).toBeInTheDocument())
    expect(client.getDivergenceCommits).toHaveBeenLastCalledWith({
      path: { repo_guid: 'repo-1' },
      query: {
        branches: 'main,develop',
        source_branch: 'main',
        target_branch: 'develop',
        page: 1,
        page_size: 50,
      },
    })
  })

  it('shows error state on API failure', async () => {
    mockGetDivergenceCommits().mockRejectedValue(new Error('Network error'))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    await waitFor(() => expect(screen.getByText('Network error')).toBeInTheDocument())
    expect(screen.getByText('Retry')).toBeInTheDocument()
  })

  it('shows 404 state when divergence not cached', async () => {
    vi.spyOn(client, 'getDivergenceCommits').mockRejectedValue(new Error('404 Not Found'))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={2}
      />
    )

    await waitFor(() =>
      expect(screen.getByText('Divergence data not found. Please run the analysis first.')).toBeInTheDocument()
    )
  })

  it('shows empty state when totalCommits is 0', async () => {
    mockGetDivergenceCommits().mockResolvedValue(mockResponse([]))

    render(
      <CommitDetailPanel
        isOpen={true}
        onClose={vi.fn()}
        repoGuid="repo-1"
        sourceBranch="main"
        targetBranch="develop"
        branches={['main', 'develop']}
        totalCommits={0}
      />
    )

    await waitFor(() =>
      expect(screen.getByText('No commits missing between these branches')).toBeInTheDocument()
    )
  })
})
