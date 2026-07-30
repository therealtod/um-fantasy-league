interface Env {
  ASSETS: { fetch(request: Request): Promise<Response> }
}

// UMFL-01 equivalent for Cloudflare Workers: public/_redirects only takes
// effect on Cloudflare Pages, so this Worker does the same same-origin
// /api/* proxy by hand. Replace BACKEND_HOST with the real backend once one
// exists at a stable (non-tunnel) hostname.
const BACKEND_HOST = 'https://unity-consider-weather-aware.trycloudflare.com'

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url)

    if (url.pathname.startsWith('/api/')) {
      const backendUrl = new URL(url.pathname + url.search, BACKEND_HOST)
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
