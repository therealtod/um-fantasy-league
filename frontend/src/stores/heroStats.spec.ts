import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { HeroStatsBoard } from '@/api/types'

vi.mock('@/api/client', () => ({
  api: { heroStats: vi.fn() },
  ApiError: class extends Error {},
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: () => [],
}))

vi.mock('@/api/sseClient', () => ({
  openStandingsStream: vi.fn(),
}))

import { api } from '@/api/client'
import { openStandingsStream } from '@/api/sseClient'
import { useHeroStatsStore } from './heroStats'

function board(tournamentId: number, points = 10): HeroStatsBoard {
  return {
    tournamentId,
    ruleSetName: 'Standard',
    currentRound: 1,
    overall: [{ rank: 1, heroId: 7, heroName: 'Bigfoot', cost: 2500, points }],
    categories: [
      {
        metric: 'WIN',
        label: 'Win',
        coefficient: 10,
        rows: [{ rank: 1, heroId: 7, heroName: 'Bigfoot', cost: 2500, points }],
      },
    ],
  }
}

describe('heroStats store', () => {
  let onUpdate: () => void
  const close = vi.fn()

  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    vi.mocked(openStandingsStream).mockImplementation((_id, cb) => {
      onUpdate = cb
      return close
    })
  })

  it('loads a tournament and opens its standings stream', async () => {
    vi.mocked(api.heroStats).mockResolvedValue(board(1))
    const store = useHeroStatsStore()

    await store.load(1)

    expect(store.board).toEqual(board(1))
    expect(store.error).toBeNull()
    expect(openStandingsStream).toHaveBeenCalledWith(1, expect.any(Function))
  })

  it('refetches the tables when the stream signals a match write', async () => {
    vi.mocked(api.heroStats).mockResolvedValueOnce(board(1)).mockResolvedValueOnce(board(1, 20))
    const store = useHeroStatsStore()
    await store.load(1)

    onUpdate()

    await vi.waitFor(() => expect(store.board?.overall[0]?.points).toBe(20))
  })

  it('closes the previous stream when switching tournament', async () => {
    vi.mocked(api.heroStats).mockImplementation(async (id) => board(id))
    const store = useHeroStatsStore()

    await store.load(1)
    await store.load(2)

    expect(close).toHaveBeenCalledTimes(1)
    expect(store.board?.tournamentId).toBe(2)
  })

  it('drops a slow response for a tournament the user has already left', async () => {
    let resolveFirst!: (value: HeroStatsBoard) => void
    vi.mocked(api.heroStats)
      .mockImplementationOnce(() => new Promise((resolve) => (resolveFirst = resolve)))
      .mockResolvedValueOnce(board(2))
    const store = useHeroStatsStore()

    const first = store.load(1)
    await store.load(2)
    resolveFirst(board(1))
    await first

    expect(store.tournamentId).toBe(2)
    expect(store.board?.tournamentId).toBe(2)
    expect(openStandingsStream).toHaveBeenCalledTimes(1)
  })

  it('surfaces a failed load as an error', async () => {
    vi.mocked(api.heroStats).mockRejectedValue(new Error('boom'))
    const store = useHeroStatsStore()

    await store.load(1)

    expect(store.board).toBeNull()
    expect(store.error).toBe('boom')
  })

  it('keeps the tables already shown when a refresh fails', async () => {
    vi.mocked(api.heroStats).mockResolvedValueOnce(board(1)).mockRejectedValueOnce(new Error('blip'))
    const store = useHeroStatsStore()
    await store.load(1)

    await store.refresh()

    expect(store.board).toEqual(board(1))
  })
})
