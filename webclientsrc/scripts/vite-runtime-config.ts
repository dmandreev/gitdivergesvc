import { readFileSync } from 'fs'
import { resolve } from 'path'
import type { Plugin, ViteDevServer } from 'vite'

function generateConfigContent(root: string): string {
  const envPath = resolve(root, '.env')
  let envText = ''
  try {
    envText = readFileSync(envPath, 'utf-8')
  } catch {
    // .env is optional; defaults below will be used
  }

  const vars = {
    API_BASE: 'http://localhost:8080',
    OIDC_AUTHORITY: 'http://localhost:8080/realms/myrealm',
    OIDC_CLIENT_ID: 'react-spa',
    USE_AUTH: 'true',
    JIRA_SERVER_ADDR: '',
  }

  for (const line of envText.split(/\r?\n/)) {
    const trimmed = line.trim()
    if (!trimmed || trimmed.startsWith('#')) continue
    const eq = trimmed.indexOf('=')
    if (eq === -1) continue
    const key = trimmed.slice(0, eq).trim()
    const value = trimmed.slice(eq + 1).trim()
    if (key === 'VITE_API_BASE') vars.API_BASE = value
    if (key === 'VITE_OIDC_AUTHORITY') vars.OIDC_AUTHORITY = value
    if (key === 'VITE_OIDC_CLIENT_ID') vars.OIDC_CLIENT_ID = value
    if (key === 'USE_AUTH') vars.USE_AUTH = value
    if (key === 'VITE_JIRA_SERVER_ADDR') vars.JIRA_SERVER_ADDR = value
  }

  return `window.__GITDIVERGE_CONFIG__ = {
  API_BASE: ${JSON.stringify(vars.API_BASE)},
  OIDC_AUTHORITY: ${JSON.stringify(vars.OIDC_AUTHORITY)},
  OIDC_CLIENT_ID: ${JSON.stringify(vars.OIDC_CLIENT_ID)},
  USE_AUTH: ${vars.USE_AUTH.toLowerCase() === 'true'},
  JIRA_SERVER_ADDR: ${JSON.stringify(vars.JIRA_SERVER_ADDR)},
}
`
}

export function runtimeConfigPlugin(): Plugin {
  return {
    name: 'runtime-config',
    configureServer(server: ViteDevServer) {
      server.middlewares.use('/config.js', (_req, res, _next) => {
        try {
          const content = generateConfigContent(server.config.root)
          res.setHeader('Content-Type', 'application/javascript')
          res.end(content)
        } catch (err: unknown) {
          res.statusCode = 500
          const message = err instanceof Error ? err.message : String(err)
          res.end(`console.error(${JSON.stringify(message)})`)
        }
      })
    },
    generateBundle(this) {
      const content = generateConfigContent(process.cwd())
      this.emitFile({
        type: 'asset',
        fileName: 'config.js',
        source: content,
      })
    },
  }
}
