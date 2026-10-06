import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Roster, RosterRole, RosterViolation, Tournament } from '@/api/types'

vi.mock('@/api/client', () => ({
  api: {
    register: vi.fn(),
    myRoster: vi.fn(),
    setSlots: vi.fn(),
    lockRoster: vi.fn(),
    swapRoster: vi.fn(),
    setRoles: vi.fn(),
    roles: vi.fn(),
  },
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
import { toRoleMap } from '@/domain/rosterRoles'
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
    currentRound: 1,
    swapsPerRound: 0,
    swapWindowOpen: false,
    rolesEnabled: false,
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
    swapWindowOpen: false,
    swapsAvailable: 0,
    alreadySwappedThisRound: false,
    swappable: false,
    roleAssignments: [],
    ...overrides,
  }
}

/**
 * Seeds a store directly at the state `adopt()` would have left it in after
 * a real load/register/toggle — `roster`, `selectedIds` and `roleOf` always
 * move together in the real store, so a test that sets only `roster` leaves
 * the other two stale.
 */
function seed(store: ReturnType<typeof useRosterStore>, heroIds: number[], overrides: Partial<Roster> = {}) {
  store.roster = roster(heroIds, overrides)
  store.selectedIds = [...heroIds]
  store.roleOf = toRoleMap(store.roster.roleAssignments)
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

  describe('the swap window', () => {
    /** A locked entry inside an open window, holding heroes 1 and 2. */
    function inWindow(store: ReturnType<typeof useRosterStore>, overrides: Partial<Roster> = {}) {
      seed(store, [1, 2], {
        locked: true,
        status: 'LOCKED',
        swapWindowOpen: true,
        swapsAvailable: 1,
        alreadySwappedThisRound: false,
        ...overrides,
      })
      store.tournamentId = TOURNAMENT_ID
    }

    it('stages a change locally instead of spending the submission on the first click', async () => {
      const store = useRosterStore()
      inWindow(store)

      await store.toggle(2)
      await store.toggle(9)

      expect(store.selectedIds).toEqual([1, 9])
      expect(store.swapsStaged).toBe(1)
      // The whole point: two clicks, still one submission left to make.
      expect(api.setSlots).not.toHaveBeenCalled()
      expect(api.swapRoster).not.toHaveBeenCalled()
    })

    it('charges nothing for staging a hero out and back in again', async () => {
      const store = useRosterStore()
      inWindow(store)

      await store.toggle(2)
      await store.toggle(2)

      expect(store.swapsStaged).toBe(0)
    })

    it('still refuses to seat more heroes than the roster holds', async () => {
      const store = useRosterStore()
      inWindow(store)

      await store.toggle(9)

      expect(store.selectedIds).toEqual([1, 2])
      expect(store.error).toContain('Drop one first')
    })

    it('submits the whole proposed roster once, and adopts the reply', async () => {
      const store = useRosterStore()
      inWindow(store)
      vi.mocked(api.swapRoster).mockResolvedValueOnce(
        roster([1, 9], {
          locked: true,
          status: 'LOCKED',
          swapWindowOpen: true,
          swapsAvailable: 0,
          alreadySwappedThisRound: true,
        }),
      )

      await store.toggle(2)
      await store.toggle(9)
      await store.submitSwaps()

      expect(api.swapRoster).toHaveBeenCalledTimes(1)
      expect(api.swapRoster).toHaveBeenCalledWith(TOURNAMENT_ID, [1, 9], [])
      expect(store.selectedIds).toEqual([1, 9])
      expect(store.alreadySwappedThisRound).toBe(true)
      expect(store.staging).toBe(false)
    })

    it('returns to the server’s roster when a submission is refused', async () => {
      const store = useRosterStore()
      inWindow(store)
      const violations: RosterViolation[] = [
        { rule: 'BUDGET_EXCEEDED', message: 'Roster costs 11900 credits' },
      ]
      vi.mocked(api.swapRoster).mockRejectedValueOnce(
        new ApiError(422, { detail: 'roster rule violated', violations }),
      )

      await store.toggle(2)
      await store.toggle(9)
      await store.submitSwaps()

      expect(store.selectedIds).toEqual([1, 2])
      expect(store.violations).toEqual(violations)
      expect(store.error).toBe('roster rule violated')
      // The submission was refused, so it was not spent: the window is still open.
      expect(store.staging).toBe(true)
    })

    it('discards a staged exchange back to the roster the server holds', async () => {
      const store = useRosterStore()
      inWindow(store)

      await store.toggle(2)
      await store.toggle(9)
      store.discardSwaps()

      expect(store.selectedIds).toEqual([1, 2])
      expect(store.swapsStaged).toBe(0)
      expect(api.swapRoster).not.toHaveBeenCalled()
    })

    it('sends nothing when there is no window to submit into', async () => {
      const store = useRosterStore()
      inWindow(store, { swapWindowOpen: false })

      await store.submitSwaps()

      expect(api.swapRoster).not.toHaveBeenCalled()
    })

    it('leaves a locked roster immutable once the round’s submission is spent', async () => {
      const store = useRosterStore()
      inWindow(store, { alreadySwappedThisRound: true })

      await store.toggle(2)

      expect(store.staging).toBe(false)
      expect(store.selectedIds).toEqual([1, 2])
      expect(api.setSlots).not.toHaveBeenCalled()
    })

    it('never stages on an unlocked entry — a draft saves on every click', async () => {
      const store = useRosterStore()
      seed(store, [1], { swapWindowOpen: true, swapsAvailable: 2 })
      store.tournamentId = TOURNAMENT_ID
      vi.mocked(api.setSlots).mockResolvedValueOnce(roster([1, 2]))

      await store.toggle(2)

      expect(store.staging).toBe(false)
      expect(api.setSlots).toHaveBeenCalledWith(TOURNAMENT_ID, [1, 2])
    })
  })

  describe('roles', () => {
    const ATTACKER = 11
    const HEALER = 12
    const ROLES: RosterRole[] = [
      {
        id: ATTACKER,
        tournamentId: TOURNAMENT_ID,
        name: 'Attacker',
        sortOrder: 1,
        weights: [{ stat: 'ATTACKS', coefficient: 1 }],
      },
      {
        id: HEALER,
        tournamentId: TOURNAMENT_ID,
        name: 'Healer',
        maxPerRoster: 1,
        sortOrder: 2,
        weights: [{ stat: 'HEALING', coefficient: 1.5 }],
      },
    ]

    /** A tournament that uses roles, with the two roles above on offer. */
    function withRoles(store: ReturnType<typeof useRosterStore>) {
      useTournamentsStore().tournaments[0]!.rolesEnabled = true
      store.tournamentId = TOURNAMENT_ID
      store.roles = ROLES
    }

    it('saves a draft role straight away and adopts the reply', async () => {
      const store = useRosterStore()
      withRoles(store)
      seed(store, [1, 2])
      vi.mocked(api.setRoles).mockResolvedValueOnce(
        roster([1, 2], { roleAssignments: [{ heroId: 1, roleId: ATTACKER }] }),
      )

      await store.setRole(1, ATTACKER)

      expect(api.setRoles).toHaveBeenCalledWith(TOURNAMENT_ID, [{ heroId: 1, roleId: ATTACKER }])
      expect(store.roleFor(1)).toBe(ATTACKER)
    })

    it('rolls the role back when the server refuses it', async () => {
      const store = useRosterStore()
      withRoles(store)
      seed(store, [1, 2])
      const violations: RosterViolation[] = [{ rule: 'ROLE_CAP_EXCEEDED', message: 'At most 1 Healer' }]
      vi.mocked(api.setRoles).mockRejectedValueOnce(new ApiError(422, { detail: 'nope', violations }))

      await store.setRole(1, HEALER)

      expect(store.roleFor(1)).toBeUndefined()
      expect(store.violations).toEqual(violations)
    })

    it('will not lock until every hero has a role, and says why', () => {
      const store = useRosterStore()
      withRoles(store)
      seed(store, [1, 2], { roleAssignments: [{ heroId: 1, roleId: ATTACKER }] })

      expect(store.lockable).toBe(false)
      expect(store.roleIssues).toEqual(['One hero still needs a role.'])

      seed(store, [1, 2], {
        roleAssignments: [
          { heroId: 1, roleId: ATTACKER },
          { heroId: 2, roleId: ATTACKER },
        ],
      })
      expect(store.lockable).toBe(true)
    })

    it('will not lock with a role over its cap', () => {
      const store = useRosterStore()
      withRoles(store)
      seed(store, [1, 2], {
        roleAssignments: [
          { heroId: 1, roleId: HEALER },
          { heroId: 2, roleId: HEALER },
        ],
      })

      expect(store.lockable).toBe(false)
      expect(store.roleIssues).toEqual(['At most 1 Healer — 2 assigned.'])
    })

    it('lets roles go unassigned in a tournament that does not use them', () => {
      const store = useRosterStore()
      store.tournamentId = TOURNAMENT_ID
      seed(store, [1, 2])

      expect(store.rolesEnabled).toBe(false)
      expect(store.roleIssues).toEqual([])
      expect(store.lockable).toBe(true)
    })

    it('sends an arriving hero’s role with the swap rather than on its own', async () => {
      const store = useRosterStore()
      withRoles(store)
      seed(store, [1, 2], {
        locked: true,
        status: 'LOCKED',
        swapWindowOpen: true,
        swapsAvailable: 1,
        roleAssignments: [
          { heroId: 1, roleId: ATTACKER },
          { heroId: 2, roleId: HEALER },
        ],
      })
      vi.mocked(api.swapRoster).mockResolvedValueOnce(roster([1, 9], { locked: true }))

      await store.toggle(2)
      await store.toggle(9)
      expect(store.roleIssues).toEqual(['One hero still needs a role.'])

      await store.setRole(9, HEALER)
      expect(api.setRoles).not.toHaveBeenCalled()
      expect(store.roleIssues).toEqual([])

      await store.submitSwaps()
      expect(api.swapRoster).toHaveBeenCalledWith(TOURNAMENT_ID, [1, 9], [
        { heroId: 9, roleId: HEALER },
      ])
    })

    it('re-assigns a kept hero on its own inside a window when nothing is staged', async () => {
      const store = useRosterStore()
      withRoles(store)
      seed(store, [1, 2], {
        locked: true,
        status: 'LOCKED',
        swapWindowOpen: true,
        swapsAvailable: 1,
        roleAssignments: [
          { heroId: 1, roleId: ATTACKER },
          { heroId: 2, roleId: HEALER },
        ],
      })
      vi.mocked(api.setRoles).mockResolvedValueOnce(roster([1, 2], { locked: true }))

      await store.setRole(2, ATTACKER)

      expect(api.setRoles).toHaveBeenCalledWith(TOURNAMENT_ID, [
        { heroId: 1, roleId: ATTACKER },
        { heroId: 2, roleId: ATTACKER },
      ])
    })

    it('leaves a locked roster’s roles alone while the window is shut', async () => {
      const store = useRosterStore()
      withRoles(store)
      seed(store, [1, 2], {
        locked: true,
        status: 'LOCKED',
        roleAssignments: [{ heroId: 1, roleId: ATTACKER }],
      })

      await store.setRole(1, HEALER)

      expect(store.rolesEditable).toBe(false)
      expect(store.roleFor(1)).toBe(ATTACKER)
      expect(api.setRoles).not.toHaveBeenCalled()
    })
  })
})
