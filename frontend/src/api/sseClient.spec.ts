import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/api/client', () => ({
  buildAuthHeaders: vi.fn().mockResolvedValue({}),
}))

import { openStandingsStream } from './sseClient'

function encode(text: string): Uint8Array {
  return new TextEncoder().encode(text)
}

/**
 * A `fetch` Response stub whose `body` yields the given chunks one `read()`
 * at a time, then signals `done`. Splitting a frame across two `chunks`
 * entries is how the tests below exercise the buffer that survives a chunk
 * boundary landing mid-line or mid-separator.
 */
function streamResponse(chunks: string[]) {
  let index = 0
  return {
    ok: true,
    status: 200,
    body: {
      getReader: () => ({
        async read() {
          if (index >= chunks.length) return { done: true, value: undefined }
          const value = encode(chunks[index])
          index += 1
          return { done: false, value }
        },
      }),
    },
  }
}

/** The `!response.ok` branch — a stream request that never opens, e.g. a 404 or 401. */
function failedResponse(status: number) {
  return { ok: false, status, body: null }
}

describe('openStandingsStream', () => {
  let fetchMock: ReturnType<typeof vi.fn>

  beforeEach(() => {
    vi.useFakeTimers()
    fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it('fires onUpdate for an event block named "update" and ignores a heartbeat comment', async () => {
    const onUpdate = vi.fn()
    fetchMock.mockResolvedValueOnce(streamResponse([': keep-alive\n\n', 'event: update\ndata: x\n\n']))

    const close = openStandingsStream(1, onUpdate)
    await vi.advanceTimersByTimeAsync(0)

    expect(onUpdate).toHaveBeenCalledTimes(1)
    close()
  })

  it('reassembles a frame whose line is split across two chunks', async () => {
    const onUpdate = vi.fn()
    fetchMock.mockResolvedValueOnce(streamResponse(['event: upda', 'te\ndata: x\n\n']))

    const close = openStandingsStream(1, onUpdate)
    await vi.advanceTimersByTimeAsync(0)

    expect(onUpdate).toHaveBeenCalledTimes(1)
    close()
  })

  it('reassembles a frame whose blank-line separator itself is split across two chunks', async () => {
    const onUpdate = vi.fn()
    fetchMock.mockResolvedValueOnce(streamResponse(['event: update\ndata: x\n', '\n']))

    const close = openStandingsStream(1, onUpdate)
    await vi.advanceTimersByTimeAsync(0)

    expect(onUpdate).toHaveBeenCalledTimes(1)
    close()
  })

  it('fires once per "update" block when several arrive in a single chunk', async () => {
    const onUpdate = vi.fn()
    fetchMock.mockResolvedValueOnce(
      streamResponse(['event: update\ndata: a\n\nevent: update\ndata: b\n\n']),
    )

    const close = openStandingsStream(1, onUpdate)
    await vi.advanceTimersByTimeAsync(0)

    expect(onUpdate).toHaveBeenCalledTimes(2)
    close()
  })

  it('never fires onUpdate for an event block with no "event: update" line', async () => {
    const onUpdate = vi.fn()
    fetchMock.mockResolvedValueOnce(streamResponse(['event: ping\ndata: still alive\n\n']))

    const close = openStandingsStream(1, onUpdate)
    await vi.advanceTimersByTimeAsync(0)

    expect(onUpdate).not.toHaveBeenCalled()
    close()
  })

  it('retries a dropped connection with capped exponential backoff', async () => {
    const onUpdate = vi.fn()
    fetchMock
      .mockRejectedValueOnce(new Error('network down'))
      .mockRejectedValueOnce(new Error('network down again'))
      .mockResolvedValueOnce(streamResponse(['event: update\ndata: x\n\n']))

    const close = openStandingsStream(1, onUpdate)

    await vi.advanceTimersByTimeAsync(0)
    expect(fetchMock).toHaveBeenCalledTimes(1)

    // First retry waits the initial 1s.
    await vi.advanceTimersByTimeAsync(1_000)
    expect(fetchMock).toHaveBeenCalledTimes(2)

    // Second retry backs off to 2s — not yet due at 1s.
    await vi.advanceTimersByTimeAsync(1_000)
    expect(fetchMock).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(1_000)
    expect(fetchMock).toHaveBeenCalledTimes(3)

    expect(onUpdate).toHaveBeenCalledTimes(1)
    close()
  })

  it('a successful connect resets backoff, so the wait after it is the initial delay again', async () => {
    const onUpdate = vi.fn()
    fetchMock
      .mockRejectedValueOnce(new Error('down')) // 1st attempt fails
      .mockResolvedValueOnce(streamResponse(['event: update\ndata: a\n\n'])) // 2nd connects, then ends
      .mockRejectedValueOnce(new Error('down again')) // 3rd attempt fails

    const close = openStandingsStream(1, onUpdate)
    await vi.advanceTimersByTimeAsync(0)
    expect(fetchMock).toHaveBeenCalledTimes(1)

    // The 1st failure backs off the initial 1s before the 2nd attempt.
    await vi.advanceTimersByTimeAsync(1_000)
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(onUpdate).toHaveBeenCalledTimes(1)

    // The 2nd attempt connected, so the wait before the 3rd is the initial
    // 1s again rather than the climbed 2s a second straight failure would owe.
    await vi.advanceTimersByTimeAsync(1_000)
    expect(fetchMock).toHaveBeenCalledTimes(3)
    close()
  })

  it('gives up without retrying on a permanent 4xx like 404', async () => {
    const onUpdate = vi.fn()
    fetchMock.mockResolvedValueOnce(failedResponse(404))

    const close = openStandingsStream(1, onUpdate)
    await vi.advanceTimersByTimeAsync(0)
    expect(fetchMock).toHaveBeenCalledTimes(1)

    // Well past every backoff tier — a retryable failure would have called
    // fetch again several times by now.
    await vi.advanceTimersByTimeAsync(120_000)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(onUpdate).not.toHaveBeenCalled()
    close()
  })

  it.each([401, 403])('gives up without retrying on a permanent %d', async (status) => {
    fetchMock.mockResolvedValueOnce(failedResponse(status))

    const close = openStandingsStream(1, vi.fn())
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(60_000)

    expect(fetchMock).toHaveBeenCalledTimes(1)
    close()
  })

  it.each([408, 429])('keeps retrying a transient %d rather than treating it as permanent', async (status) => {
    const onUpdate = vi.fn()
    fetchMock
      .mockResolvedValueOnce(failedResponse(status))
      .mockResolvedValueOnce(streamResponse(['event: update\ndata: x\n\n']))

    const close = openStandingsStream(1, onUpdate)
    await vi.advanceTimersByTimeAsync(0)
    expect(fetchMock).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(1_000)
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(onUpdate).toHaveBeenCalledTimes(1)
    close()
  })

  it('stops retrying once closed, even if a retry was already scheduled', async () => {
    fetchMock.mockRejectedValue(new Error('network down'))

    const close = openStandingsStream(1, vi.fn())
    await vi.advanceTimersByTimeAsync(0)
    expect(fetchMock).toHaveBeenCalledTimes(1)

    close()
    await vi.advanceTimersByTimeAsync(60_000)

    // No further attempt once stopped, however long the clock runs.
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })
})
