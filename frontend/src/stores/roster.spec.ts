import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Roster, RosterViolation, Tournament } from '@/api/types'

vi.mock('@/api/client', () => ({
  api: { register: vi.fn(), myRoster: vi.fn(), setSlots: vi.fn(), lockRoster: vi.fn() },
  ApiError: class extends Error {
    constructor(
      readonly status: number,
      readonly problem: { detail?: string; violations?: RosterViolation[] } = {},
    ) {
      super(problem.detail ?? `Request failed with status ${status}`)
    }

    get violations() {
      return this.problem.violations ?? []
    }
  },
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
}))

import { api, ApiError } from '@/api/client'
import { useHeroesStore } from './heroes'
import { useRosterStore } from './roster'
import { useTournamentsStore } from './tournaments'

const TOURNAMENT_ID = 7

function tournament(overrides: Partial<Tournament> = {}): Tournament {
  return {
    id: TOURNAMENT_ID,
    name: 'Autumn Championship',
    format: 'BANQUEST',
    status: 'REGISTRATION_OPEN',
    startDate: '2026-09-01',
    capacity: 8,
    enrolled: 1,
    rosterSize: 2,
    creditGrant: 10_000,
    acceptsRegistration: true,
    ...overrides,
  }
}

function roster(heroIds: number[], overrides: Partial<Roster> = {}): Roster {
  const heroes = heroIds.map((id) => ({ id, name: `Hero ${id}`, imageUrl: null, cost: 1_000 }))
  const spent = heroes.reduce((sum, h) => sum + h.cost, 0)
  return {
    entryId: 1,
    tournamentId: TOURNAMENT_ID,
    tournamentName: 'Autumn Championship',
    status: 'DRAFT',
    locked: false,
    lockedAt: null,
    rosterSize: 2,
    heroes,
    budget: { spent, creditGrant: 10_000, remaining: 10_000 - spent, utilisation: spent / 10_000 },
    lockable: false,
    ...overrides,
  }
}

/**
 * Seeds a store directly at the state `adopt()` would have left it in after
 * a real load/register/toggle — `roster` and `selectedIds` always move
 * together in the real store, so a test that sets only `roster` leaves
 * `selectedIds` stale.
 */
function seed(store: ReturnType<typeof useRosterStore>, heroIds: number[], overrides: Partial<Roster> = {}) {
  store.roster = roster(heroIds, overrides)
  store.selectedIds = [...heroIds]
}

describe('roster store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    useTournamentsStore().tournaments.push(tournament())
  })

  describe('toggle', () => {
    it('adds a hero optimistically, then adopts the server roster on success', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      seed(store, [])

      const { promise, resolve } = (() => {
        let resolveFn!: (value: Roster) => void
        const p = new Promise<Roster>((res) => {
          resolveFn = res
        })
        return { promise: p, resolve: resolveFn }
      })()
      vi.mocked(api.setSlots).mockReturnValueOnce(promise)

      const toggling = store.toggle(1)
      // Optimistic: the id is selected before the server has replied.
      expect(store.selectedIds).toEqual([1])

      resolve(roster([1]))
      await toggling

      expect(store.selectedIds).toEqual([1])
      expect(store.error).toBeNull()
    })

    it('rolls the optimistic selection back when the server rejects the change', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      seed(store, [1])

      const violations: RosterViolation[] = [{ rule: 'OVER_BUDGET', message: 'Over budget' }]
      vi.mocked(api.setSlots).mockRejectedValueOnce(new ApiError(422, { detail: 'roster rule violated', violations }))

      await store.toggle(2)

      expect(store.selectedIds).toEqual([1])
      expect(store.violations).toEqual(violations)
      expect(store.error).toBe('roster rule violated')
    })

    it('rolls back and reports a generic message for a non-ApiError failure', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      seed(store, [1])

      vi.mocked(api.setSlots).mockRejectedValueOnce(new Error('network down'))

      await store.toggle(2)

      expect(store.selectedIds).toEqual([1])
      expect(store.error).toBe('Could not update roster')
    })

    it('removes an already-selected hero', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      seed(store, [1, 2])

      vi.mocked(api.setSlots).mockResolvedValueOnce(roster([2]))
      await store.toggle(1)

      expect(api.setSlots).toHaveBeenCalledWith(TOURNAMENT_ID, [2])
      expect(store.selectedIds).toEqual([2])
    })

    it('refuses to add past the roster size and never calls the API', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      seed(store, [1, 2]) // rosterSize is 2 — already full

      await store.toggle(3)

      expect(api.setSlots).not.toHaveBeenCalled()
      expect(store.selectedIds).toEqual([1, 2])
      expect(store.error).toBe('Roster holds 2 heroes. Drop one first.')
    })

    it('is a no-op with no tournament selected', async () => {
      const store = useRosterStore()

      await store.toggle(1)

      expect(api.setSlots).not.toHaveBeenCalled()
      expect(store.selectedIds).toEqual([])
    })

    it('is a no-op once the roster is locked', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      seed(store, [1], { locked: true })

      await store.toggle(2)

      expect(api.setSlots).not.toHaveBeenCalled()
    })

    it('is a no-op before the manager has registered', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID // pointed at a tournament, but no entry yet

      await store.toggle(1)

      expect(api.setSlots).not.toHaveBeenCalled()
      expect(store.selectedIds).toEqual([])
    })
  })

  describe('load', () => {
    it('treats a 404 as "not registered" rather than an error', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      vi.mocked(api.myRoster).mockRejectedValueOnce(new ApiError(404, {}))

      await store.load()

      expect(store.error).toBeNull()
      expect(store.roster).toBeNull()
      expect(store.registered).toBe(false)
    })

    it('surfaces any other failure as an error', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      vi.mocked(api.myRoster).mockRejectedValueOnce(new Error('boom'))

      await store.load()

      expect(store.error).toBe('boom')
    })

    it('does nothing with no tournament selected', async () => {
      const store = useRosterStore()

      await store.load()

      expect(api.myRoster).not.toHaveBeenCalled()
    })
  })

  describe('select', () => {
    it('resets state and loads the new tournament\'s roster', async () => {
      const store = useRosterStore()
      store.roster = roster([1])
      store.violations = [{ rule: 'OVER_BUDGET', message: 'x' }]
      vi.mocked(api.myRoster).mockResolvedValueOnce(roster([9]))

      await store.select(TOURNAMENT_ID)

      expect(store.tournamentId).toBe(TOURNAMENT_ID)
      expect(store.violations).toEqual([])
      expect(store.selectedIds).toEqual([9])
    })

    it('resets without loading when passed null', async () => {
      const store = useRosterStore()
      store.roster = roster([1])

      await store.select(null)

      expect(store.tournamentId).toBeNull()
      expect(store.roster).toBeNull()
      expect(api.myRoster).not.toHaveBeenCalled()
    })
  })

  describe('register', () => {
    it('adopts the new entry and refreshes the tournaments list', async () => {
      const store = useRosterStore()
      const tournamentsStore = useTournamentsStore()
      const loadSpy = vi.spyOn(tournamentsStore, 'load').mockResolvedValue()
      store.tournamentId = TOURNAMENT_ID
      vi.mocked(api.register).mockResolvedValueOnce(roster([]))

      await store.register()

      expect(store.registered).toBe(true)
      expect(loadSpy).toHaveBeenCalledTimes(1)
    })

    it('reports a failure without touching registered state', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      vi.mocked(api.register).mockRejectedValueOnce(new Error('tournament is full'))

      await store.register()

      expect(store.error).toBe('tournament is full')
      expect(store.registered).toBe(false)
    })
  })

  describe('lock', () => {
    it('adopts the locked roster on success', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      seed(store, [1, 2])
      vi.mocked(api.lockRoster).mockResolvedValueOnce(roster([1, 2], { locked: true, status: 'LOCKED' }))

      await store.lock()

      expect(store.locked).toBe(true)
      expect(store.violations).toEqual([])
    })

    it('surfaces violations on rejection without locking', async () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      seed(store, [1])
      const violations: RosterViolation[] = [{ rule: 'ROSTER_INCOMPLETE', message: 'Pick 2 heroes' }]
      vi.mocked(api.lockRoster).mockRejectedValueOnce(new ApiError(422, { detail: 'roster rule violated', violations }))

      await store.lock()

      expect(store.locked).toBe(false)
      expect(store.violations).toEqual(violations)
    })
  })

  describe('budget and lockable', () => {
    it('derives spend from the selected heroes, resolved against the loaded pool', () => {
      const store = useRosterStore()
      const heroesStore = useHeroesStore()
      heroesStore.heroes = [
        { id: 1, name: 'Alice', imageUrl: null, cost: 3_000 },
        { id: 2, name: 'Medusa', imageUrl: null, cost: 4_000 },
      ]
      seed(store, [1, 2])

      expect(store.budget.spent).toBe(7_000)
      expect(store.budget.remaining).toBe(3_000)
    })

    it('is lockable only once registered, unlocked, full and within budget', () => {
      const store = useRosterStore()
      const heroesStore = useHeroesStore()
      heroesStore.heroes = [
        { id: 1, name: 'Alice', imageUrl: null, cost: 3_000 },
        { id: 2, name: 'Medusa', imageUrl: null, cost: 4_000 },
      ]
      seed(store, [1])
      expect(store.lockable).toBe(false) // not full yet

      seed(store, [1, 2])
      expect(store.lockable).toBe(true)

      seed(store, [1, 2], { locked: true })
      expect(store.lockable).toBe(false) // already locked
    })
  })
})
