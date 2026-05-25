import { render, screen } from '@testing-library/react'
import { ProgressBar } from './ProgressBar'

describe('ProgressBar', () => {
  it('renders nothing when no progress', () => {
    const { container } = render(<ProgressBar />)
    expect(container.firstChild).toBeNull()
  })

  it('shows start progress with 0% when total is known', () => {
    render(<ProgressBar progress={{ event: 'start', total: 10, message: 'Cloning' }} />)
    expect(screen.getByText('Cloning')).toBeInTheDocument()
    const bar = screen.getByRole('progressbar')
    expect(bar).toHaveAttribute('aria-valuenow', '0')
  })

  it('shows indeterminate animation when total is null', () => {
    render(<ProgressBar progress={{ event: 'start', total: undefined, message: 'Fetching origin' }} />)
    expect(screen.getByText('Fetching origin')).toBeInTheDocument()
    const bar = screen.getByRole('progressbar')
    expect(bar).not.toHaveAttribute('aria-valuenow')
  })

  it('shows advance progress with correct percentage', () => {
    render(<ProgressBar progress={{ event: 'advance', current: 3, total: 10, message: 'Fetching' }} />)
    expect(screen.getByText('Fetching')).toBeInTheDocument()
    expect(screen.getByText('3 / 10')).toBeInTheDocument()
    const bar = screen.getByRole('progressbar')
    expect(bar).toHaveAttribute('aria-valuenow', '30')
  })

  it('shows finish progress at 100%', () => {
    render(<ProgressBar progress={{ event: 'finish', message: 'Done' }} />)
    expect(screen.getByText('Done')).toBeInTheDocument()
    const bar = screen.getByRole('progressbar')
    expect(bar).toHaveAttribute('aria-valuenow', '100')
  })
})
