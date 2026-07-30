import { buildAuthHeaders } from './client'

const INITIAL_RETRY_DELAY_MS = 1000
const MAX_RETRY_DELAY_MS = 30_000

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** True for an SSE event block that carries a named "update" event. */
function isUpdateEvent(rawEvent: string): boolean {
  return rawEvent
    .split('\n')
    .some((line) => line.startsWith('event:') && line.slice('event:'.length).trim() === 'update')
}

/**
 * Subscribes to a tournament's `/standings/stream` SSE endpoint and calls
 * `onUpdate` whenever the backend signals that a match was recorded,
 * corrected, or deleted. The stream carries no payload beyond that signal —
 * the caller already knows how to pull fresh data (see `standings.ts`'s
 * `refresh()`).
 *
 * Built on `fetch` + a `ReadableStream` reader rather than `EventSource`
 * because `EventSource` cannot set an `Authorization` header, and this app
 * has no session cookie to fall back on. That also means there's no free
 * auto-reconnect, so a dropped connection (server restart, network blip, or
 * the backend's own periodic heartbeat lapsing) is retried here with capped
 * exponential backoff.
 *
 * Returns a cleanup function that closes the connection and stops retrying.
 */
export function openStandingsStream(tournamentId: number, onUpdate: () => void): () => void {
  const controller = new AbortController()
  let stopped = false
  let retryDelayMs = INITIAL_RETRY_DELAY_MS

  async function connectOnce() {
    const headers = await buildAuthHeaders()
    const response = await fetch(`/api/tournaments/${tournamentId}/standings/stream`, {
      headers: { ...headers, Accept: 'text/event-stream' },
      signal: controller.signal,
    })
    if (!response.ok || !response.body) {
      throw new Error(`standings stream request failed with status ${response.status}`)
    }

    retryDelayMs = INITIAL_RETRY_DELAY_MS // a successful connect resets backoff

    const reader = response.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''

    while (!stopped) {
      const { done, value } = await reader.read()
      if (done) return
      buffer += decoder.decode(value, { stream: true })

      let boundary = buffer.indexOf('\n\n')
      while (boundary !== -1) {
        const rawEvent = buffer.slice(0, boundary)
        buffer = buffer.slice(boundary + 2)
        if (isUpdateEvent(rawEvent)) onUpdate()
        boundary = buffer.indexOf('\n\n')
      }
    }
  }

  async function run() {
    while (!stopped) {
      try {
        await connectOnce()
      } catch {
        // Connection dropped or never opened — fall through to backoff/retry below.
      }
      if (stopped) return
      await sleep(retryDelayMs)
      retryDelayMs = Math.min(retryDelayMs * 2, MAX_RETRY_DELAY_MS)
    }
  }

  void run()

  return () => {
    stopped = true
    controller.abort()
  }
}
