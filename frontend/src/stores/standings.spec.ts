import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { StandingsBoard, TickerEntry } from '@/api/types'

vi.mock('@/api/client', () => ({
  api: { standings: vi.fn(), matches: vi.fn() },
  ApiError: class extends Error {},
}))

vi.mock('@/api/sseClient', () => ({
  openStandingsStream: vi.fn(),
}))

import { api } from '@/api/client'
import { openStandingsStream } from '@/api/sseClient'
import { useStandingsStore } from './standings'

const board: StandingsBoard = {
  tournamentId: 1,
  ruleSetName: 'Standard',
  currentRound: 1,
  metrics: [],
  rows: [],
}

function entry(matchId: number, points: number): TickerEntry {
  return {
    matchId,
    round: 1,
    mapName: 'Camelot',
    playedAt: '2026-08-01T00:00:00Z',
    sides: [
      { playerLabel: 'Alice', heroName: 'Alice', healthRemaining: 10, isWinner: true, points },
    ],
    bannedHeroNames: [],
  }
}

describe('standings store', () => {
  let onUpdate: () => void

  beforeEach(async () => {
    setActivePinia(createPinia())
    vi.mocked(openStandingsStream).mockImplementation((_id, cb) => {
      onUpdate = cb
      return () => {}
    })
    vi.mocked(api.standings).mockResolvedValue(board)
    vi.mocked(api.matches).mockResolvedValue([entry(1, 5), entry(2, 3)])

    const standings = useStandingsStore()
    await standings.load(1)
  })

  it('refetches the board on an SSE update even when matches returns no new rows', async () => {
    const standings = useStandingsStore()
    const updatedBoard: StandingsBoard = { ...board, currentRound: 2 }
    vi.mocked(api.matches).mockResolvedValueOnce([])
    vi.mocked(api.standings).mockResolvedValueOnce(updatedBoard)

    onUpdate()
    await vi.waitFor(() => expect(standings.board).toEqual(updatedBoard))
  })

  it('reflects a correction to an existing match id rather than only appending new ones', async () => {
    const standings = useStandingsStore()
    vi.mocked(api.matches).mockResolvedValueOnce([entry(1, 99), entry(2, 3)])

    onUpdate()
    await vi.waitFor(() =>
      expect(standings.ticker.find((e) => e.matchId === 1)?.sides[0].points).toBe(99),
    )
  })

  it('removes a deleted match from the ticker', async () => {
    const standings = useStandingsStore()
    expect(standings.ticker.map((e) => e.matchId)).toEqual([1, 2])
    vi.mocked(api.matches).mockResolvedValueOnce([entry(2, 3)])

    onUpdate()
    await vi.waitFor(() => expect(standings.ticker.map((e) => e.matchId)).toEqual([2]))
  })
})
