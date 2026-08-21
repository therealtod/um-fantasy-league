import { buildAuthHeaders } from './client'

const INITIAL_RETRY_DELAY_MS = 1000
const MAX_RETRY_DELAY_MS = 30_000

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** A connect attempt that got an HTTP response, just not one that opened the stream. */
class StreamRequestError extends Error {
  constructor(readonly status: number) {
    super(`standings stream request failed with status ${status}`)
  }
}

/**
 * A 4xx other than 408 (timeout) or 429 (rate limit) is not going to start
 * succeeding on its own — a mistyped or deleted tournament id, or a stream
 * this manager isn't authorised for, stays exactly as wrong on the next
 * attempt. Retrying it forever would just poll the backend every
 * `MAX_RETRY_DELAY_MS` for as long as the tab stays open.
 */
function isPermanentFailure(status: number): boolean {
  return status >= 400 && status < 500 && status !== 408 && status !== 429
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
 * exponential backoff — except a permanent 4xx (see `isPermanentFailure`),
 * which gives up instead of polling a broken tournament id or a forbidden
 * stream forever.
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
      throw new StreamRequestError(response.status)
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
      } catch (e) {
        if (e instanceof StreamRequestError && isPermanentFailure(e.status)) return
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
