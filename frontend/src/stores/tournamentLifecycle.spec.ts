import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Roster, StandingsBoard, TickerEntry } from '@/api/types'

vi.mock('@/api/client', () => ({
  api: {
    tournaments: vi.fn().mockResolvedValue([]),
    register: vi.fn(),
    setSlots: vi.fn(),
    lockRoster: vi.fn(),
    standings: vi.fn(),
    matches: vi.fn(),
  },
  ApiError: class extends Error {
    status = 0
    violations: unknown[] = []
  },
}))

import { api } from '@/api/client'
import { useRosterStore } from './roster'
import { useStandingsStore } from './standings'

const TOURNAMENT_ID = 42

/**
 * A full manager journey through the roster store — register, click three
 * heroes into the three slots one at a time (exactly what the Roster
 * Builder UI does), then lock — against a fresh Pinia so two managers never
 * share state, the same way two browser sessions wouldn't.
 *
 * `@/api/client` is the frontend's one seam onto the network (see
 * `src/api/client.ts`), so mocking it here is the frontend equivalent of the
 * backend's service-layer integration test: every store, computed and
 * optimistic-rollback path in between runs for real.
 */
async function draftAndLock(
  handle: string,
  picks: { id: number; name: string; cost: number }[],
): Promise<ReturnType<typeof useRosterStore>> {
  setActivePinia(createPinia())
  const roster = useRosterStore()

  const draft = (heroes: typeof picks): Roster => {
    const spent = heroes.reduce((sum, h) => sum + h.cost, 0)
    return {
      entryId: 100,
      tournamentId: TOURNAMENT_ID,
      tournamentName: 'Autumn Championship',
      status: 'DRAFT',
      locked: false,
      lockedAt: null,
      rosterSize: 3,
      heroes: heroes.map((h) => ({ id: h.id, name: h.name, imageUrl: null, cost: h.cost })),
      budget: { spent, creditGrant: 10_000, remaining: 10_000 - spent, utilisation: spent / 10_000 },
      lockable: heroes.length === 3,
    }
  }

  vi.mocked(api.register).mockResolvedValueOnce(draft([]))
  await roster.select(TOURNAMENT_ID)
  await roster.register()
  expect(roster.registered).toBe(true)
  expect(roster.locked).toBe(false)

  for (let i = 1; i <= picks.length; i++) {
    vi.mocked(api.setSlots).mockResolvedValueOnce(draft(picks.slice(0, i)))
    await roster.toggle(picks[i - 1].id)
  }

  expect(roster.full).toBe(true)
  expect(roster.selected.map((h) => h.name)).toEqual(picks.map((p) => p.name))
  expect(roster.budget.spent).toBe(picks.reduce((sum, p) => sum + p.cost, 0))
  expect(roster.lockable).toBe(true)

  const locked: Roster = { ...draft(picks), status: 'LOCKED', locked: true, lockedAt: '2026-10-01T00:00:00Z', lockable: false }
  vi.mocked(api.lockRoster).mockResolvedValueOnce(locked)
  await roster.lock()

  expect(roster.locked).toBe(true)
  expect(roster.error, `${handle}'s lock should not surface an error`).toBeNull()

  return roster
}

describe('tournament lifecycle (frontend stores)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.tournaments).mockResolvedValue([])
  })

  it('two managers draft and lock independently, then the standings board names a winner', async () => {
    const winnerRoster = await draftAndLock('SherlockMain', [
      { id: 1, name: 'Alice', cost: 1_000 },
      { id: 2, name: 'King Arthur', cost: 1_000 },
      { id: 3, name: 'Robin Hood', cost: 1_000 },
    ])
    const runnerUpRoster = await draftAndLock('MythicMind', [
      { id: 4, name: 'Medusa', cost: 1_000 },
      { id: 5, name: 'Sherlock Holmes', cost: 1_000 },
      { id: 6, name: 'Dracula', cost: 1_000 },
    ])

    // Locking one manager's roster must never have touched the other's.
    expect(winnerRoster.selected.map((h) => h.name)).toEqual(['Alice', 'King Arthur', 'Robin Hood'])
    expect(runnerUpRoster.selected.map((h) => h.name)).toEqual(['Medusa', 'Sherlock Holmes', 'Dracula'])

    // The tournament has since gone LIVE and finished; the admin recorded
    // results and the backend derived this board (mirrors
    // TournamentLifecycleIntegrationTest on the backend).
    const board: StandingsBoard = {
      tournamentId: TOURNAMENT_ID,
      ruleSetName: 'Autumn Standard',
      currentRound: 2,
      metrics: [
        { metric: 'WIN', label: 'Win', coefficient: 10 },
        { metric: 'HEALTH_REMAINING', label: 'Health Remaining', coefficient: 1 },
        { metric: 'APPEARANCE', label: 'Appearance', coefficient: 1 },
      ],
      rows: [
        {
          rank: 1,
          entryId: 100,
          managerId: 1,
          handle: 'SherlockMain',
          displayName: 'Sherlock Main',
          roster: ['Alice', 'King Arthur', 'Robin Hood'],
          spent: 3_000,
          creditGrant: 10_000,
          totalPoints: 40,
          roundPoints: 21,
          breakdown: { WIN: 20, HEALTH_REMAINING: 18, APPEARANCE: 2 },
        },
        {
          rank: 2,
          entryId: 101,
          managerId: 2,
          handle: 'MythicMind',
          displayName: 'Mythic Mind',
          roster: ['Medusa', 'Sherlock Holmes', 'Dracula'],
          spent: 3_000,
          creditGrant: 10_000,
          totalPoints: 4,
          roundPoints: 3,
          breakdown: { WIN: 0, HEALTH_REMAINING: 2, APPEARANCE: 2 },
        },
      ],
    }
    const ticker: TickerEntry[] = [
      {
        matchId: 4,
        round: 2,
        playedAt: '2026-10-02T00:00:00Z',
        games: [
          {
            gameNumber: 1,
            mapName: 'Raptor Paddock',
            sides: [
              { playerLabel: 'Aurelie Blanc', heroName: 'King Arthur', healthRemaining: 10, isWinner: true, points: 21 },
              { playerLabel: 'Miles Ashworth', heroName: 'Sherlock Holmes', healthRemaining: 2, isWinner: false, points: 3 },
            ],
          },
        ],
        bannedHeroNames: [],
      },
    ]

    setActivePinia(createPinia())
    const standings = useStandingsStore()
    vi.mocked(api.standings).mockResolvedValueOnce(board)
    vi.mocked(api.matches).mockResolvedValueOnce(ticker)

    await standings.load(TOURNAMENT_ID)

    expect(standings.error).toBeNull()
    expect(standings.rows.map((r) => r.handle)).toEqual(['SherlockMain', 'MythicMind'])
    expect(standings.rows[0].rank).toBe(1)
    expect(standings.rows[0].totalPoints).toBeGreaterThan(standings.rows[1].totalPoints)
    expect(standings.highestMatchId).toBe(4)
  })
})
