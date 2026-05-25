import { render, screen } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import App from './App'

vi.hoisted(() => {
  window.__GITDIVERGE_CONFIG__ = { USE_AUTH: true }
})

vi.mock('./auth/AuthProvider', async () => {
  const actual = await vi.importActual<typeof import('./auth/AuthProvider')>('./auth/AuthProvider')
  return {
    ...actual,
    useAuth: () => ({
      user: null,
      isLoading: false,
      signinRedirect: vi.fn(),
      signoutRedirect: vi.fn(),
      getAccessToken: () => null,
    }),
    AuthProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  }
})

describe('App', () => {
  it('renders landing page when unauthenticated', () => {
    render(
      <MemoryRouter initialEntries={['/']}>
        <Routes>
          <Route path="/*" element={<App />} />
        </Routes>
      </MemoryRouter>
    )
    expect(screen.getByText('Branch Divergence Analytics')).toBeInTheDocument()
    expect(screen.getByText(/Sign in to start analysing/)).toBeInTheDocument()
  })
})
