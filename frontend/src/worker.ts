interface Env {
  ASSETS: { fetch(request: Request): Promise<Response> }
  // Backend base URL — not hardcoded here and not in wrangler.toml either,
  // since it's currently a rotating tunnel hostname; set directly on the
  // Worker in the Cloudflare dashboard (see wrangler.toml) so it never lands
  // in the public repo. Keep it in step with VITE_API_PROXY_TARGET /
  // server.proxy in vite.config.ts, which points dev at the same API.
  BACKEND_HOST: string
}

// The frontend deploys as a Worker with static assets, not a Pages project, so
// a public/_redirects rule would never be read — this does the same-origin
// /api/* proxy by hand. That proxy is why prod needs no CORS allowlist; see
// ProdCorsConfig on the backend.
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url)

    if (url.pathname.startsWith('/api/')) {
      const backendUrl = new URL(url.pathname + url.search, env.BACKEND_HOST)
      return fetch(new Request(backendUrl, request))
    }

    const assetResponse = await env.ASSETS.fetch(request)
    if (assetResponse.status === 404) {
      // Vue Router history mode: let unknown paths fall through to the SPA shell.
      return env.ASSETS.fetch(new Request(new URL('/index.html', url), request))
    }
    return assetResponse
  },
}
