declare global {
  interface Window {
    __GITDIVERGE_CONFIG__?: {
      API_BASE?: string
      OIDC_AUTHORITY?: string
      OIDC_CLIENT_ID?: string
      USE_AUTH?: boolean | string
      JIRA_SERVER_ADDR?: string
    }
  }
}

const runtime = window.__GITDIVERGE_CONFIG__ ?? {}

export const CONFIG = {
  API_BASE: runtime.API_BASE ?? 'http://localhost:8080',
  OIDC_AUTHORITY: runtime.OIDC_AUTHORITY ?? 'http://localhost:8080/realms/myrealm',
  OIDC_CLIENT_ID: runtime.OIDC_CLIENT_ID ?? 'react-spa',
  USE_AUTH: runtime.USE_AUTH !== false && runtime.USE_AUTH !== 'false',
  JIRA_SERVER_ADDR: runtime.JIRA_SERVER_ADDR ?? '',
} as const
