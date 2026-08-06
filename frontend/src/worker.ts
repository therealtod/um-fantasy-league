interface Env {
  ASSETS: { fetch(request: Request): Promise<Response> }
  // Backend base URL — set in wrangler.toml [vars], not hardcoded here, so a
  // tunnel rotation (or the eventual move to a stable host) is a one-line
  // change instead of a code edit. See BACKEND_HOST in AGENTS.md's CI/CD
  // section for the equivalent placeholder on the Pages/_redirects side.
  BACKEND_HOST: string
}

// UMFL-01 equivalent for Cloudflare Workers: public/_redirects only takes
// effect on Cloudflare Pages, so this Worker does the same same-origin
// /api/* proxy by hand.
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
