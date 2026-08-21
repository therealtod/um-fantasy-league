import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent } from 'vue'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/api/client', () => ({
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: () => [],
}))

vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({ tournaments: [{ id: 1, name: 'Winter Open' }] }),
}))

import { usePoolManagement } from './usePoolManagement'

interface Item {
  id: number
  name: string
}

/** usePoolManagement calls onMounted, so it needs a real component instance. */
function setUp(config: Parameters<typeof usePoolManagement<Item, Item>>[0]) {
  let result!: ReturnType<typeof usePoolManagement<Item, Item>>
  mount(
    defineComponent({
      setup() {
        result = usePoolManagement<Item, Item>(config)
        return () => null
      },
    }),
  )
  return result
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((res) => {
    resolve = res
  })
  return { promise, resolve }
}

describe('usePoolManagement', () => {
  it('loads the catalog on mount', async () => {
    const catalog: Item[] = [{ id: 1, name: 'Alpha' }]
    const { promise, resolve } = deferred<Item[]>()
    const pool = setUp({
      entityLabel: 'Item',
      loadCatalog: () => promise,
      loadPool: vi.fn(),
      removeFromPool: vi.fn(),
    })

    expect(pool.loading.value).toBe(true)
    resolve(catalog)
    await promise
    await flushPromises()

    expect(pool.catalog.value).toEqual(catalog)
    expect(pool.loading.value).toBe(false)
  })

  it('surfaces the entity-labelled fallback when the catalog load fails', async () => {
    const pool = setUp({
      entityLabel: 'Map',
      loadCatalog: () => Promise.reject(new Error('boom')),
      loadPool: vi.fn(),
      removeFromPool: vi.fn(),
    })
    await flushPromises()

    expect(pool.error.value).toBe('boom')
  })

  it('loads the pool when a tournament is selected, and clears it when deselected', async () => {
    const loadPool = vi.fn().mockResolvedValue([{ id: 1, name: 'Board' }])
    const pool = setUp({
      entityLabel: 'Map',
      loadCatalog: () => Promise.resolve([]),
      loadPool,
      removeFromPool: vi.fn(),
    })

    pool.selectedTournamentId.value = 5
    await flushPromises()

    expect(loadPool).toHaveBeenCalledWith(5)
    expect(pool.pool.value).toEqual([{ id: 1, name: 'Board' }])

    pool.selectedTournamentId.value = null
    await flushPromises()

    expect(pool.pool.value).toEqual([])
  })

  it('drops a pool load superseded by a later tournament switch', async () => {
    const first = deferred<Item[]>()
    const second = deferred<Item[]>()
    const loadPool = vi
      .fn()
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise)
    const pool = setUp({
      entityLabel: 'Map',
      loadCatalog: () => Promise.resolve([]),
      loadPool,
      removeFromPool: vi.fn(),
    })

    pool.selectedTournamentId.value = 1
    await flushPromises()
    pool.selectedTournamentId.value = 2
    await flushPromises()

    second.resolve([{ id: 2, name: 'Second' }])
    await second.promise
    await flushPromises()
    expect(pool.pool.value).toEqual([{ id: 2, name: 'Second' }])

    // The first tournament's load resolving late must not clobber the second's result.
    first.resolve([{ id: 1, name: 'First' }])
    await first.promise
    await flushPromises()
    expect(pool.pool.value).toEqual([{ id: 2, name: 'Second' }])
  })

  it('removes an item from the pool and reloads it once confirmed', async () => {
    const removeFromPool = vi.fn().mockResolvedValue(undefined)
    const loadPool = vi
      .fn()
      .mockResolvedValueOnce([{ id: 1, name: 'Board' }])
      .mockResolvedValueOnce([])
    const pool = setUp({
      entityLabel: 'Map',
      loadCatalog: () => Promise.resolve([]),
      loadPool,
      removeFromPool,
    })

    pool.selectedTournamentId.value = 1
    await flushPromises()

    pool.startRemove(pool.pool.value[0])
    expect(pool.removingItem.value).toEqual({ id: 1, name: 'Board' })

    await pool.confirmRemove()
    await flushPromises()

    expect(removeFromPool).toHaveBeenCalledWith(1, 1)
    expect(pool.removingItem.value).toBeNull()
    expect(pool.pool.value).toEqual([])
  })

  it('cancelling a removal leaves the pool untouched', async () => {
    const removeFromPool = vi.fn()
    const pool = setUp({
      entityLabel: 'Map',
      loadCatalog: () => Promise.resolve([]),
      loadPool: vi.fn().mockResolvedValue([{ id: 1, name: 'Board' }]),
      removeFromPool,
    })
    pool.selectedTournamentId.value = 1
    await flushPromises()

    pool.startRemove(pool.pool.value[0])
    pool.cancelRemove()

    expect(pool.removingItem.value).toBeNull()
    expect(removeFromPool).not.toHaveBeenCalled()
  })

  it('clears a pending removal when the tournament changes', async () => {
    const pool = setUp({
      entityLabel: 'Map',
      loadCatalog: () => Promise.resolve([]),
      loadPool: vi.fn().mockResolvedValue([{ id: 1, name: 'Board' }]),
      removeFromPool: vi.fn(),
    })
    pool.selectedTournamentId.value = 1
    await flushPromises()
    pool.startRemove(pool.pool.value[0])

    pool.selectedTournamentId.value = 2
    await flushPromises()

    expect(pool.removingItem.value).toBeNull()
  })

  it('reports a failed removal with the entity-labelled fallback, without dropping the item', async () => {
    const pool = setUp({
      entityLabel: 'Map',
      loadCatalog: () => Promise.resolve([]),
      loadPool: vi.fn().mockResolvedValue([{ id: 1, name: 'Board' }]),
      removeFromPool: vi.fn().mockRejectedValue(new Error('Map is used by a recorded match')),
    })
    pool.selectedTournamentId.value = 1
    await flushPromises()

    pool.startRemove(pool.pool.value[0])
    await pool.confirmRemove()
    await flushPromises()

    expect(pool.error.value).toBe('Map is used by a recorded match')
  })
})
