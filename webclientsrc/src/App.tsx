import { Routes, Route } from 'react-router-dom'
import { useState } from 'react'
import { AuthProvider, useAuth } from './auth/AuthProvider'
import { Header } from './components/Header'
import { RepoList } from './components/RepoList'
import { CloneRepo } from './components/CloneRepo'
import { AnalysisPanel } from './components/AnalysisPanel'
import { Callback } from './pages/Callback'
import { BulkAnalysis } from './pages/BulkAnalysis'
import { CONFIG } from './config'
import type { RepoIndexEntry } from './api/client'
import { GitBranch, BarChart3, ArrowLeft } from 'lucide-react'

function Main() {
  const { user } = useAuth()
  const [selectedRepo, setSelectedRepo] = useState<RepoIndexEntry | null>(null)
  const [refreshKey, setRefreshKey] = useState(0)

  const isAuthenticated = CONFIG.USE_AUTH ? !!user : true

  return (
    <div className="min-h-screen bg-surface-0 text-text-main">
      <Header />
      <main className="max-w-6xl mx-auto px-6 py-8">
        {!isAuthenticated ? (
          <div className="flex flex-col items-center justify-center py-24 gap-6 text-center">
            <div className="w-20 h-20 rounded-2xl bg-gradient-to-br from-accent to-accent-light flex items-center justify-center shadow-2xl shadow-accent-glow">
              <BarChart3 className="w-10 h-10 text-white" />
            </div>
            <div className="space-y-2 max-w-md">
              <h2 className="text-2xl font-semibold text-text-main tracking-tight">
                Branch Divergence Analytics
              </h2>
              <p className="text-sm text-text-muted leading-relaxed">
                Visualise commit gaps between branches across your repositories.
                Sign in to start analysing pairwise branch divergence.
              </p>
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-1 lg:grid-cols-12 gap-8">
            {/* Sidebar */}
            <aside className="lg:col-span-4 space-y-6">
              <CloneRepo onCloned={() => setRefreshKey((k) => k + 1)} />
              <div className="h-px bg-surface-3/60" />
              <RepoList
                refreshSignal={refreshKey}
                onSelect={setSelectedRepo}
                selectedGuid={selectedRepo?.guid}
                onDelete={(repo) => {
                  if (selectedRepo?.guid === repo.guid) {
                    setSelectedRepo(null)
                  }
                }}
              />
            </aside>

            {/* Main panel */}
            <section className="lg:col-span-8 space-y-6">
              {selectedRepo ? (
                <div className="space-y-6">
                  <div className="flex items-center gap-3">
                    <button
                      onClick={() => setSelectedRepo(null)}
                      className="p-1.5 rounded-md text-text-dim hover:text-text-main hover:bg-surface-2 transition-colors"
                      aria-label="Back to overview"
                    >
                      <ArrowLeft className="w-4 h-4" />
                    </button>
                    <div className="w-8 h-8 rounded-lg bg-accent/10 flex items-center justify-center">
                      <GitBranch className="w-4 h-4 text-accent-light" />
                    </div>
                    <div>
                      <h2 className="text-base font-semibold text-text-main leading-tight">
                        {selectedRepo.name}
                      </h2>
                    </div>
                  </div>
                  <AnalysisPanel repo={selectedRepo} />
                </div>
              ) : (
                <div className="flex flex-col items-center justify-center py-24 gap-5 text-center border border-dashed border-surface-3 rounded-2xl bg-surface-1/30">
                  <div className="w-14 h-14 rounded-xl bg-surface-2 flex items-center justify-center">
                    <GitBranch className="w-7 h-7 text-text-dim" />
                  </div>
                  <div className="space-y-1 max-w-sm">
                    <p className="text-sm font-medium text-text-muted">
                      Select a repository
                    </p>
                    <p className="text-xs text-text-dim">
                      Choose a repository from the sidebar to run branch divergence analysis.
                    </p>
                  </div>
                </div>
              )}
            </section>
          </div>
        )}
      </main>

      <footer className="border-t border-surface-3 mt-auto">
        <div className="max-w-6xl mx-auto px-6 py-4 flex items-center justify-between text-[11px] text-text-dim">
          <span>GitDiverge Client</span>
        </div>
      </footer>
    </div>
  )
}

export default function App() {
  return (
    <AuthProvider>
      <Routes>
        <Route path="/auth/callback" element={<Callback />} />
        <Route path="/bulk" element={<BulkAnalysis />} />
        <Route path="/*" element={<Main />} />
      </Routes>
    </AuthProvider>
  )
}
