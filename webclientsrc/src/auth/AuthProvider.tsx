import { createContext, useContext, useEffect, useState, useCallback } from 'react'
import { User, UserManager } from 'oidc-client-ts'
import { oidcConfig } from './oidcConfig'
import { CONFIG } from '../config'

const userManager = CONFIG.USE_AUTH ? new UserManager(oidcConfig) : null

export interface AuthContextValue {
  user: User | null
  isLoading: boolean
  signinRedirect: () => Promise<void>
  signoutRedirect: () => Promise<void>
  getAccessToken: () => string | null
}

const AuthContext = createContext<AuthContextValue | null>(null)

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [user, setUser] = useState<User | null>(null)
  const [isLoading, setIsLoading] = useState(CONFIG.USE_AUTH)

  useEffect(() => {
    if (!CONFIG.USE_AUTH || !userManager) {
      setIsLoading(false)
      return
    }

    let mounted = true
    userManager
      .getUser()
      .then((u) => {
        if (!mounted) return
        setUser(u ?? null)
        setIsLoading(false)
      })
      .catch(() => {
        if (!mounted) return
        setIsLoading(false)
      })

    const onUserLoaded = (u: User) => setUser(u)
    const onUserUnloaded = () => setUser(null)
    const onAccessTokenExpired = () => {
      console.warn('Access token expired')
      setUser(null)
      userManager.removeUser().catch(() => {})
    }
    const onSilentRenewError = (err: Error) => {
      console.error('Silent renew error:', err)
    }

    userManager.events.addUserLoaded(onUserLoaded)
    userManager.events.addUserUnloaded(onUserUnloaded)
    userManager.events.addAccessTokenExpired(onAccessTokenExpired)
    userManager.events.addSilentRenewError(onSilentRenewError)

    return () => {
      mounted = false
      userManager.events.removeUserLoaded(onUserLoaded)
      userManager.events.removeUserUnloaded(onUserUnloaded)
      userManager.events.removeAccessTokenExpired(onAccessTokenExpired)
      userManager.events.removeSilentRenewError(onSilentRenewError)
    }
  }, [])

  const signinRedirect = useCallback(async () => {
    if (userManager) await userManager.signinRedirect()
  }, [])
  const signoutRedirect = useCallback(async () => {
    if (userManager) await userManager.signoutRedirect()
  }, [])
  const getAccessToken = useCallback(() => user?.access_token ?? null, [user])

  return (
    <AuthContext.Provider
      value={{ user, isLoading, signinRedirect, signoutRedirect, getAccessToken }}
    >
      {children}
    </AuthContext.Provider>
  )
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used inside AuthProvider')
  return ctx
}

export { userManager }
