import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { DivergenceMatrix } from './DivergenceMatrix'
import type { BranchAnalyticsSummary, BranchStatus } from '../api/client'

const mockAnalytics: BranchAnalyticsSummary = {
  repo_path: '/tmp/test-repo',
  branches: ['main', 'develop', 'feature'],
  comparisons: [
    { source_branch: 'develop', target_branch: 'main', missing_commit_count: 0 },
    { source_branch: 'feature', target_branch: 'main', missing_commit_count: 3 },
    { source_branch: 'main', target_branch: 'develop', missing_commit_count: 1 },
    { source_branch: 'feature', target_branch: 'develop', missing_commit_count: 12 },
    { source_branch: 'main', target_branch: 'feature', missing_commit_count: 25 },
    { source_branch: 'develop', target_branch: 'feature', missing_commit_count: 0 },
  ],
}

const mockBranchStatuses: BranchStatus[] = [
  { branch: 'main', exists: true },
  { branch: 'develop', exists: true },
  { branch: 'feature', exists: true },
  { branch: 'nonexistent', exists: false },
]

function renderMatrix(onCellClick = vi.fn()) {
  return render(
    <DivergenceMatrix
      analytics={mockAnalytics}
      branchStatuses={mockBranchStatuses}
      onCellClick={onCellClick}
    />
  )
}

describe('DivergenceMatrix', () => {
  it('renders branch statuses', () => {
    renderMatrix()
    const badges = screen.getAllByText(/^(main|develop|feature|nonexistent)$/)
    expect(badges.length).toBeGreaterThanOrEqual(4)
  })

  it('renders green badge for existing branches and red badge for missing branches', () => {
    renderMatrix()

    const mainBadge = screen.getByText('main', { selector: '.rounded-full' })
    expect(mainBadge).toHaveClass('bg-green/10')
    expect(mainBadge).toHaveClass('text-green')

    const missingBadge = screen.getByText('nonexistent', { selector: '.rounded-full' })
    expect(missingBadge).toHaveClass('bg-red/10')
    expect(missingBadge).toHaveClass('text-red')
  })

  it('renders zero commit cell for develop→main', () => {
    renderMatrix()
    const cell = screen.getByTitle('develop → main: 0 missing commits')
    expect(cell).toBeInTheDocument()
    expect(cell.textContent).toContain('0')
  })

  it('renders green cell for 1-9 commits (feature→main has 3)', () => {
    renderMatrix()
    const cell = screen.getByTitle('feature → main: 3 missing commits')
    expect(cell).toBeInTheDocument()
    expect(cell.textContent).toContain('3')
  })

  it('renders yellow cell for 10-20 commits (feature→develop has 12)', () => {
    renderMatrix()
    const cell = screen.getByTitle('feature → develop: 12 missing commits')
    expect(cell).toBeInTheDocument()
    expect(cell.textContent).toContain('12')
  })

  it('renders red cell for 21+ commits (main→feature has 25)', () => {
    renderMatrix()
    const cell = screen.getByTitle('main → feature: 25 missing commits')
    expect(cell).toBeInTheDocument()
    expect(cell.textContent).toContain('25')
  })

  it('renders legend', () => {
    renderMatrix()
    expect(screen.getByText('Legend')).toBeInTheDocument()
    const zeros = screen.getAllByText('0')
    expect(zeros.length).toBeGreaterThanOrEqual(1)
    expect(screen.getByText('1–9')).toBeInTheDocument()
    expect(screen.getByText('10–20')).toBeInTheDocument()
    expect(screen.getByText('21+')).toBeInTheDocument()
  })

  it('calls onCellClick when clicking a cell with commits', async () => {
    const user = userEvent.setup()
    const onCellClick = vi.fn()
    renderMatrix(onCellClick)

    const cell = screen.getByTitle('feature → main: 3 missing commits')
    await user.click(cell)

    expect(onCellClick).toHaveBeenCalledWith('feature', 'main', 3)
  })

  it('does not call onCellClick for zero-commit cells', async () => {
    const user = userEvent.setup()
    const onCellClick = vi.fn()
    renderMatrix(onCellClick)

    const cell = screen.getByTitle('develop → main: 0 missing commits')
    await user.click(cell)

    expect(onCellClick).not.toHaveBeenCalled()
  })
})
