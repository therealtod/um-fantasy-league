import { ref } from 'vue'
import { describeError, violationMessages } from '@/api/client'

export type AsyncResult<T> = { ok: true; value: T } | { ok: false }

/**
 * The `loading`/`error`/`violations` triple and the surrounding
 * `try/catch/finally` that every store and admin wizard wraps around a fetch
 * or a save, plus the "drop an out-of-order response" guard a subset of them
 * re-derive by hand — a monotonic counter in `heroes.ts`, a comparison
 * against the ref that the request was for in `standings.ts` and the pool
 * wizards. One instance is normally enough per component/store: `run()`
 * supersedes any call still in flight on the *same* instance, so an earlier
 * call's eventual success or failure is dropped instead of clobbering
 * fresher state — matching what the hand-rolled guards already did, just as
 * a single mechanism instead of one re-derived per call site.
 *
 * `run()` returns `{ ok: false }` on failure *or* on being superseded,
 * rather than a bare `T | undefined`, because several endpoints in this app
 * genuinely resolve to `undefined` on success (`removeHeroFromPool`,
 * `removeMapFromPool`, …) — a `result === undefined` check would misread
 * that success as a drop.
 */
export function useAsyncRequest() {
  const loading = ref(false)
  const error = ref<string | null>(null)
  const violations = ref<string[]>([])

  // Bumped on every run() so a call superseded by a newer one on this same
  // instance — started later but not necessarily finishing later — can be
  // told apart and dropped instead of writing stale state or a stale error.
  let token = 0

  async function run<T>(fn: () => Promise<T>, fallback: string): Promise<AsyncResult<T>> {
    const thisToken = ++token
    loading.value = true
    error.value = null
    violations.value = []
    try {
      const value = await fn()
      if (thisToken !== token) return { ok: false }
      return { ok: true, value }
    } catch (e) {
      if (thisToken !== token) return { ok: false }
      error.value = describeError(e, fallback)
      violations.value = violationMessages(e)
      return { ok: false }
    } finally {
      if (thisToken === token) loading.value = false
    }
  }

  /** Clears any error/violations left over from a previous run(), without touching loading. */
  function reset() {
    error.value = null
    violations.value = []
  }

  return { loading, error, violations, run, reset }
}
