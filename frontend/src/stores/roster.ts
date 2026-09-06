import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { api, ApiError, describeError } from '@/api/client'
import type { BudgetStatus, Hero, Roster, RosterViolation } from '@/api/types'
import { budgetStatus as computeBudgetStatus, heroesChanged } from '@/domain/rosterPolicy'
import { standingsAvailable } from '@/domain/tournamentStatus'
import { useHeroesStore } from './heroes'
import { useTournamentsStore } from './tournaments'

export const useRosterStore = defineStore('roster', () => {
  const heroesStore = useHeroesStore()
  const tournamentsStore = useTournamentsStore()

  const tournamentId = ref<number | null>(null)
  const roster = ref<Roster | null>(null)
  /** Optimistic local selection, so the budget meter reacts before the server replies. */
  const selectedIds = ref<number[]>([])
  const loading = ref(false)
  const saving = ref(false)
  const error = ref<string | null>(null)
  const violations = ref<RosterViolation[]>([])

  const tournament = computed(() =>
    tournamentId.value === null ? null : tournamentsStore.byId(tournamentId.value),
  )

  /** Whether the standings page would let this tournament be selected yet. */
  const standingsOpen = computed(() => standingsAvailable(tournament.value?.status))

  const rosterSize = computed(() => roster.value?.rosterSize ?? tournament.value?.rosterSize ?? 3)
  /** The budget granted at registration; the entry's snapshot wins over the tournament's. */
  const creditGrant = computed(
    () => roster.value?.budget.creditGrant ?? tournament.value?.creditGrant ?? 10_000,
  )
  const registered = computed(() => roster.value !== null)
  const locked = computed(() => roster.value?.locked ?? false)

  const swapWindowOpen = computed(() => roster.value?.swapWindowOpen ?? false)
  const swapsAvailable = computed(() => roster.value?.swapsAvailable ?? 0)
  const alreadySwappedThisRound = computed(() => roster.value?.alreadySwappedThisRound ?? false)

  /**
   * Whether edits are being **staged** rather than saved on every click.
   *
   * This is the whole reason the swap path is not just `toggle` with a
   * different endpoint. A draft PUTs the entire roster on each click, which is
   * free because a draft can be re-saved forever; a window grants exactly one
   * submission, so the first click would spend it and leave the manager with a
   * half-made exchange. While this is true `toggle` mutates the selection and
   * sends nothing, and `submitSwaps` is the single call that spends the round.
   */
  const staging = computed(
    () =>
      registered.value &&
      locked.value &&
      swapWindowOpen.value &&
      swapsAvailable.value > 0 &&
      !alreadySwappedThisRound.value,
  )

  /**
   * How many heroes the staged selection would bring in — what a submission
   * would cost. Counted against the roster the *server* last confirmed, not
   * against an earlier staging step, so toggling a hero off and back on again
   * costs nothing.
   */
  const swapsStaged = computed(() =>
    heroesChanged(roster.value?.heroes.map((hero) => hero.id) ?? [], selectedIds.value),
  )

  /** The selected heroes, resolved against the loaded pool, in slot order. */
  const selected = computed<Hero[]>(() =>
    selectedIds.value
      .map((id) => heroesStore.byId(id) ?? roster.value?.heroes.find((h) => h.id === id) ?? null)
      .filter((hero): hero is Hero => hero !== null),
  )

  /** Computed locally for instant feedback; the server's copy is authoritative. */
  const budget = computed<BudgetStatus>(() =>
    computeBudgetStatus(
      selected.value.map((hero) => hero.cost),
      creditGrant.value,
    ),
  )

  const full = computed(() => selectedIds.value.length === rosterSize.value)
  const lockable = computed(
    () => registered.value && !locked.value && full.value && budget.value.spent <= creditGrant.value,
  )

  function isSelected(heroId: number) {
    return selectedIds.value.includes(heroId)
  }

  function reset() {
    roster.value = null
    selectedIds.value = []
    violations.value = []
    error.value = null
  }

  /**
   * Drops the whole session's roster state, including which tournament it was
   * for. Called on sign-out — `reset()` alone leaves `tournamentId` set, which
   * would make `RosterBuilderView`'s "already selected" guard skip re-fetching
   * for a manager who signs in after and lands on the same tournament's roster.
   */
  function clearSession() {
    tournamentId.value = null
    reset()
  }

  function adopt(next: Roster) {
    roster.value = next
    selectedIds.value = next.heroes.map((hero) => hero.id)
    violations.value = []
  }

  async function select(id: number | null) {
    tournamentId.value = id
    reset()
    if (id !== null) await load()
  }

  async function load() {
    if (tournamentId.value === null) return
    loading.value = true
    error.value = null
    try {
      adopt(await api.myRoster(tournamentId.value))
    } catch (e) {
      // A 404 simply means "not registered yet" — not an error worth showing.
      if (e instanceof ApiError && e.status === 404) {
        roster.value = null
        selectedIds.value = []
      } else {
        error.value = describeError(e, 'Could not load roster')
      }
    } finally {
      loading.value = false
    }
  }

  async function register() {
    if (tournamentId.value === null) return
    saving.value = true
    error.value = null
    try {
      adopt(await api.register(tournamentId.value))
      await tournamentsStore.load()
    } catch (e) {
      error.value = describeError(e, 'Registration failed')
    } finally {
      saving.value = false
    }
  }

  /**
   * Add or remove a hero.
   *
   * The local selection updates first so the grid and budget meter respond
   * immediately; if the server rejects the change, the previous selection is
   * restored.
   */
  async function toggle(heroId: number) {
    const id = tournamentId.value
    if (id === null || !registered.value) return
    // A locked roster is immutable *except* inside an open window, and then
    // only locally — see `staging`.
    if (locked.value && !staging.value) return

    const previous = [...selectedIds.value]
    const next = isSelected(heroId)
      ? previous.filter((selectedId) => selectedId !== heroId)
      : [...previous, heroId]

    if (next.length > rosterSize.value) {
      error.value = `Roster holds ${rosterSize.value} heroes. Drop one first.`
      return
    }

    selectedIds.value = next
    error.value = null
    violations.value = []

    // Staged: the exchange is not sent until `submitSwaps`, so there is no
    // request to roll back and nothing has been spent.
    if (staging.value) return

    saving.value = true
    try {
      adopt(await api.setSlots(id, next))
    } catch (e) {
      selectedIds.value = previous
      if (e instanceof ApiError) {
        violations.value = e.violations
        error.value = e.message
      } else {
        error.value = 'Could not update roster'
      }
    } finally {
      saving.value = false
    }
  }

  /**
   * Send the staged roster as this round's one exchange.
   *
   * The whole proposed roster goes up, not a list of pairs: the server derives
   * the diff, so the two sides cannot disagree about what counts as one swap.
   * A rejection restores the server's roster rather than leaving the staged
   * selection in place — the violations say what was wrong, and the manager
   * still has the submission, so the honest starting point is where they were.
   */
  async function submitSwaps() {
    const id = tournamentId.value
    if (id === null || !staging.value) return
    saving.value = true
    error.value = null
    violations.value = []
    try {
      adopt(await api.swapRoster(id, selectedIds.value))
      await tournamentsStore.load()
    } catch (e) {
      if (roster.value) selectedIds.value = roster.value.heroes.map((hero) => hero.id)
      if (e instanceof ApiError) {
        violations.value = e.violations
        error.value = e.message
      } else {
        error.value = 'Could not submit swaps'
      }
    } finally {
      saving.value = false
    }
  }

  /** Abandon a staged exchange and go back to the roster the server holds. */
  function discardSwaps() {
    if (!roster.value) return
    selectedIds.value = roster.value.heroes.map((hero) => hero.id)
    error.value = null
    violations.value = []
  }

  async function lock() {
    if (tournamentId.value === null) return
    saving.value = true
    error.value = null
    violations.value = []
    try {
      adopt(await api.lockRoster(tournamentId.value))
    } catch (e) {
      if (e instanceof ApiError) {
        violations.value = e.violations
        error.value = e.message
      } else {
        error.value = 'Could not lock roster'
      }
    } finally {
      saving.value = false
    }
  }

  return {
    tournamentId,
    tournament,
    standingsOpen,
    roster,
    selectedIds,
    selected,
    loading,
    saving,
    error,
    violations,
    rosterSize,
    creditGrant,
    registered,
    locked,
    budget,
    full,
    lockable,
    swapWindowOpen,
    swapsAvailable,
    alreadySwappedThisRound,
    staging,
    swapsStaged,
    isSelected,
    select,
    load,
    register,
    toggle,
    lock,
    submitSwaps,
    discardSwaps,
    clearSession,
  }
})
