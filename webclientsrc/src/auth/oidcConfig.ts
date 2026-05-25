import type { UserManagerSettings } from 'oidc-client-ts'
import { WebStorageStateStore } from 'oidc-client-ts'
import { CONFIG } from '../config'

export const oidcConfig: UserManagerSettings = {
  authority: CONFIG.OIDC_AUTHORITY,
  client_id: CONFIG.OIDC_CLIENT_ID,
  redirect_uri: `${window.location.origin}/auth/callback`,
  post_logout_redirect_uri: window.location.origin,
  silent_redirect_uri: `${window.location.origin}/silent-renew.html`,
  response_type: 'code',
  scope: 'openid profile email',
  automaticSilentRenew: true,
  accessTokenExpiringNotificationTimeInSeconds: 10,
  userStore: new WebStorageStateStore({ store: window.localStorage }),
  metadata: {
    issuer: CONFIG.OIDC_AUTHORITY,
    authorization_endpoint: `${CONFIG.OIDC_AUTHORITY}/protocol/openid-connect/auth`,
    token_endpoint: `${CONFIG.OIDC_AUTHORITY}/protocol/openid-connect/token`,
    userinfo_endpoint: `${CONFIG.OIDC_AUTHORITY}/protocol/openid-connect/userinfo`,
    end_session_endpoint: `${CONFIG.OIDC_AUTHORITY}/protocol/openid-connect/logout`,
  },
}
