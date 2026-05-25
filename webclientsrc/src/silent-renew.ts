import { UserManager } from 'oidc-client-ts'
import { oidcConfig } from './auth/oidcConfig'

new UserManager({ ...oidcConfig, automaticSilentRenew: false })
  .signinSilentCallback()
  .catch((err) => {
    console.error('Silent renew callback error:', err)
  })
