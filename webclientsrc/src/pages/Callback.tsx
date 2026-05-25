import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { userManager } from '../auth/AuthProvider'
import { CONFIG } from '../config'
import { Loader2 } from 'lucide-react'

export function Callback() {
  const navigate = useNavigate()

  useEffect(() => {
    if (!CONFIG.USE_AUTH || !userManager) {
      navigate('/', { replace: true })
      return
    }

    const url = window.location.href
    // Strip auth params immediately so a React StrictMode remount can't reuse them
    window.history.replaceState({}, document.title, window.location.pathname)

    const hasCode = new URL(url).searchParams.has('code')
    if (!hasCode) {
      navigate('/', { replace: true })
      return
    }

    userManager
      .signinRedirectCallback(url)
      .then(() => navigate('/', { replace: true }))
      .catch((err) => {
        console.error('OIDC callback error:', err)
        navigate('/', { replace: true })
      })
  }, [navigate])

  return (
    <div className="min-h-screen flex flex-col items-center justify-center gap-4 text-text-muted">
      <Loader2 className="w-8 h-8 animate-spin text-accent" />
      <p className="text-sm font-medium">Completing sign-in…</p>
    </div>
  )
}
