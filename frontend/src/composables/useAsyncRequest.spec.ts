import { describe, expect, it, vi } from 'vitest'

vi.mock('@/api/client', () => ({
  ApiError: class ApiError extends Error {
    constructor(
      readonly status: number,
      readonly problem: { detail?: string; violations?: { rule: string; message: string }[] },
    ) {
      super(problem.detail)
    }

    get violations() {
      return this.problem.violations ?? []
    }
  },
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: (e: unknown) =>
    e instanceof Error && 'violations' in e
      ? (e as { violations: { message: string }[] }).violations.map((v) => v.message)
      : [],
}))

import { ApiError } from '@/api/client'
import { useAsyncRequest } from './useAsyncRequest'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('useAsyncRequest', () => {
  it('tracks loading around a successful call and returns its value', async () => {
    const { loading, error, run } = useAsyncRequest()
    const { promise, resolve } = deferred<string>()

    const pending = run(() => promise, 'fallback')
    expect(loading.value).toBe(true)

    resolve('ok')
    const result = await pending

    expect(result).toEqual({ ok: true, value: 'ok' })
    expect(loading.value).toBe(false)
    expect(error.value).toBeNull()
  })

  it('reports a thrown Error message and clears loading', async () => {
    const { loading, error, run } = useAsyncRequest()

    const result = await run(() => Promise.reject(new Error('boom')), 'fallback')

    expect(result).toEqual({ ok: false })
    expect(loading.value).toBe(false)
    expect(error.value).toBe('boom')
  })

  it('falls back to the given message for a non-Error rejection', async () => {
    const { error, run } = useAsyncRequest()

    await run(() => Promise.reject('not an Error'), 'Could not do the thing')

    expect(error.value).toBe('Could not do the thing')
  })

  it('surfaces ApiError violations as display strings', async () => {
    const { error, violations, run } = useAsyncRequest()
    const apiError = new ApiError(422, {
      detail: 'Roster invalid',
      violations: [{ rule: 'OVER_BUDGET', message: 'Over budget' }],
    })

    await run(() => Promise.reject(apiError), 'fallback')

    expect(error.value).toBe('Roster invalid')
    expect(violations.value).toEqual(['Over budget'])
  })

  it('clears a previous error/violations at the start of a new run', async () => {
    const { error, violations, run } = useAsyncRequest()
    await run(() => Promise.reject(new Error('first failure')), 'fallback')
    expect(error.value).toBe('first failure')

    const { promise, resolve } = deferred<void>()
    const pending = run(() => promise, 'fallback')
    expect(error.value).toBeNull()
    expect(violations.value).toEqual([])

    resolve()
    await pending
  })

  it('drops a superseded call: an earlier run() resolving after a later one does not overwrite state', async () => {
    const { loading, error, run } = useAsyncRequest()
    const first = deferred<string>()
    const second = deferred<string>()

    const firstResult = run(() => first.promise, 'fallback')
    const secondResult = run(() => second.promise, 'fallback')

    second.resolve('second')
    await secondResult
    expect(loading.value).toBe(false)

    first.resolve('first')
    expect(await firstResult).toEqual({ ok: false })
    // The superseded call's late success must not flip loading back on nor
    // report an error for the call that actually won.
    expect(loading.value).toBe(false)
    expect(error.value).toBeNull()
  })

  it('drops a superseded call that fails, rather than reporting its error over the winner', async () => {
    const { error, run } = useAsyncRequest()
    const first = deferred<string>()
    const second = deferred<string>()

    const firstResult = run(() => first.promise, 'first fallback')
    const secondResult = run(() => second.promise, 'second fallback')

    second.resolve('second')
    await secondResult
    expect(error.value).toBeNull()

    first.reject(new Error('late failure'))
    expect(await firstResult).toEqual({ ok: false })
    expect(error.value).toBeNull()
  })

  it('reports success with an undefined value rather than treating it as a drop', async () => {
    const { run } = useAsyncRequest()

    const result = await run<void>(() => Promise.resolve(undefined), 'fallback')

    expect(result).toEqual({ ok: true, value: undefined })
  })

  it('reset() clears error/violations without touching loading', async () => {
    const { loading, error, violations, run, reset } = useAsyncRequest()
    await run(() => Promise.reject(new Error('boom')), 'fallback')
    expect(error.value).toBe('boom')

    reset()

    expect(error.value).toBeNull()
    expect(violations.value).toEqual([])
    expect(loading.value).toBe(false)
  })
})
