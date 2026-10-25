import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { HallOfFame } from '@/api/types'

vi.mock('@/api/client', () => ({
  api: { hallOfFame: vi.fn() },
  ApiError: class extends Error {},
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: () => [],
}))

import { api } from '@/api/client'
import { useHallOfFameStore } from './hallOfFame'

const HALL: HallOfFame = {
  titles: [{ rank: 1, managerId: 1, handle: 'NeonStrategist', displayName: 'Neon', titles: 1, podiums: 1 }],
  tournaments: [],
}

describe('hallOfFame store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(api.hallOfFame).mockReset()
  })

  it('holds what the server returned', async () => {
    vi.mocked(api.hallOfFame).mockResolvedValue(HALL)
    const store = useHallOfFameStore()

    await store.load()

    expect(store.hallOfFame).toEqual(HALL)
    expect(store.error).toBeNull()
    expect(store.loading).toBe(false)
  })

  it('keeps the last good result and reports a failed reload', async () => {
    vi.mocked(api.hallOfFame).mockResolvedValueOnce(HALL).mockRejectedValueOnce(new Error('offline'))
    const store = useHallOfFameStore()

    await store.load()
    await store.load()

    expect(store.hallOfFame).toEqual(HALL)
    expect(store.error).toBe('offline')
  })
})
