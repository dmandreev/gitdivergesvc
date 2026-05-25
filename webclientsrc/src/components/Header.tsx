import { GitBranch, LogOut, LogIn, Activity } from 'lucide-react'
import { NavLink } from 'react-router-dom'
import { useAuth } from '../auth/AuthProvider'
import { CONFIG } from '../config'

export function Header() {
  const { user, isLoading, signinRedirect, signoutRedirect } = useAuth()

  return (
    <header className="border-b border-surface-3 bg-surface-1/80 backdrop-blur-md sticky top-0 z-50">
      <div className="max-w-6xl mx-auto px-6 h-16 flex items-center justify-between">
        <div className="flex items-center gap-6">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-gradient-to-br from-accent to-accent-light flex items-center justify-center shadow-lg shadow-accent-glow">
              <GitBranch className="w-5 h-5 text-white" />
            </div>
            <div>
              <h1 className="text-lg font-semibold tracking-tight text-text-main leading-tight">
                GitDiverge
              </h1>
              <p className="text-[11px] text-text-dim leading-tight tracking-wide uppercase">
                Branch Analytics
              </p>
            </div>
          </div>

          <nav className="hidden sm:flex items-center gap-1">
            <NavLink
              to="/"
              end
              className={({ isActive }) =>
                `px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${
                  isActive
                    ? 'text-accent-light bg-accent/10'
                    : 'text-text-muted hover:text-text-main hover:bg-surface-2'
                }`
              }
            >
              Repositories
            </NavLink>
            <NavLink
              to="/bulk"
              className={({ isActive }) =>
                `px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${
                  isActive
                    ? 'text-accent-light bg-accent/10'
                    : 'text-text-muted hover:text-text-main hover:bg-surface-2'
                }`
              }
            >
              Bulk Analysis
            </NavLink>
          </nav>
        </div>

        <div className="flex items-center gap-3">
          {CONFIG.USE_AUTH && !isLoading && (
            <>
              {user ? (
                <>
                  <div className="hidden sm:flex items-center gap-2 px-3 py-1.5 rounded-full bg-surface-2 border border-surface-3">
                    <Activity className="w-3.5 h-3.5 text-green" />
                    <span className="text-xs text-text-muted font-medium">
                      {user.profile?.preferred_username ?? user.profile?.sub ?? 'Authenticated'}
                    </span>
                  </div>
                  <button
                    onClick={() => signoutRedirect()}
                    className="flex items-center gap-2 px-4 py-2 rounded-lg text-xs font-medium text-text-muted hover:text-text-main hover:bg-surface-2 transition-colors"
                  >
                    <LogOut className="w-3.5 h-3.5" />
                    Sign out
                  </button>
                </>
              ) : (
                <button
                  onClick={() => signinRedirect()}
                  className="flex items-center gap-2 px-4 py-2 rounded-lg text-xs font-semibold text-white bg-gradient-to-r from-accent to-accent-light hover:opacity-90 transition-opacity shadow-lg shadow-accent-glow"
                >
                  <LogIn className="w-3.5 h-3.5" />
                  Sign in
                </button>
              )}
            </>
          )}
        </div>
      </div>
    </header>
  )
}
