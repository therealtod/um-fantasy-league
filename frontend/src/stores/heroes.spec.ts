import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Hero } from '@/api/types'

vi.mock('@/api/client', () => ({
  api: { heroes: vi.fn() },
  ApiError: class extends Error {
    violations: unknown[] = []
  },
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: () => [],
}))

import { api } from '@/api/client'
import { useHeroesStore } from './heroes'

function hero(id: number, name: string, cost = 1_000): Hero {
  return { id, name, imageUrl: null, cost }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((res) => {
    resolve = res
  })
  return { promise, resolve }
}

describe('heroes store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('load(id) points the pool at that tournament and stores the result', async () => {
    const heroes = useHeroesStore()
    vi.mocked(api.heroes).mockResolvedValueOnce([hero(1, 'Alice')])

    await heroes.load(42)

    expect(heroes.tournamentId).toBe(42)
    expect(heroes.heroes).toEqual([hero(1, 'Alice')])
    expect(api.heroes).toHaveBeenCalledWith(42, { sort: 'COST', search: undefined })
  })

  it('load(null) clears the pool without calling the API', async () => {
    const heroes = useHeroesStore()
    vi.mocked(api.heroes).mockResolvedValueOnce([hero(1, 'Alice')])
    await heroes.load(42)

    await heroes.load(null)

    expect(heroes.tournamentId).toBeNull()
    expect(heroes.heroes).toEqual([])
    expect(api.heroes).toHaveBeenCalledTimes(1)
  })

  it('select(id) is a no-op when the pool already points at that tournament', async () => {
    const heroes = useHeroesStore()
    vi.mocked(api.heroes).mockResolvedValueOnce([hero(1, 'Alice')])
    await heroes.load(42)

    await heroes.select(42)

    expect(api.heroes).toHaveBeenCalledTimes(1)
    expect(heroes.heroes).toEqual([hero(1, 'Alice')])
  })

  it('select(id) clears the current pool and reloads when the tournament changes', async () => {
    const heroes = useHeroesStore()
    vi.mocked(api.heroes).mockResolvedValueOnce([hero(1, 'Alice')])
    await heroes.load(42)

    const { promise, resolve } = deferred<Hero[]>()
    vi.mocked(api.heroes).mockReturnValueOnce(promise)
    const pending = heroes.select(99)

    // Cleared immediately, before the new tournament's pool has arrived.
    expect(heroes.heroes).toEqual([])

    resolve([hero(2, 'Medusa')])
    await pending

    expect(heroes.tournamentId).toBe(99)
    expect(heroes.heroes).toEqual([hero(2, 'Medusa')])
  })

  it('drops an earlier load() that resolves after a newer one for the same store', async () => {
    const heroes = useHeroesStore()
    const first = deferred<Hero[]>()
    const second = deferred<Hero[]>()
    vi.mocked(api.heroes).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)

    const firstLoad = heroes.load(1)
    const secondLoad = heroes.load(1)

    second.resolve([hero(2, 'Medusa')])
    await secondLoad
    expect(heroes.heroes).toEqual([hero(2, 'Medusa')])

    // The stale request finally resolves — it must not clobber the winner.
    first.resolve([hero(1, 'Alice')])
    await firstLoad
    expect(heroes.heroes).toEqual([hero(2, 'Medusa')])
    expect(heroes.loading).toBe(false)
  })

  it('surfaces a load failure via describeError and leaves the pool untouched', async () => {
    const heroes = useHeroesStore()
    vi.mocked(api.heroes).mockRejectedValueOnce(new Error('boom'))

    await heroes.load(1)

    expect(heroes.error).toBe('boom')
    expect(heroes.heroes).toEqual([])
  })

  it('byId finds a loaded hero and returns null for an unknown id', async () => {
    const heroes = useHeroesStore()
    vi.mocked(api.heroes).mockResolvedValueOnce([hero(1, 'Alice'), hero(2, 'Medusa')])
    await heroes.load(1)

    expect(heroes.byId(2)?.name).toBe('Medusa')
    expect(heroes.byId(999)).toBeNull()
  })
})
