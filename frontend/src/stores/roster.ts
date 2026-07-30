import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { api, ApiError } from '@/api/client'
import type { BudgetStatus, Hero, Roster, RosterViolation } from '@/api/types'
import { budgetStatus as computeBudgetStatus } from '@/domain/rosterPolicy'
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

  const rosterSize = computed(() => roster.value?.rosterSize ?? tournament.value?.rosterSize ?? 3)
  /** The budget granted at registration; the entry's snapshot wins over the tournament's. */
  const creditGrant = computed(
    () => roster.value?.budget.creditGrant ?? tournament.value?.creditGrant ?? 10_000,
  )
  const registered = computed(() => roster.value !== null)
  const locked = computed(() => roster.value?.locked ?? false)

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
        error.value = e instanceof Error ? e.message : 'Could not load roster'
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
      error.value = e instanceof Error ? e.message : 'Registration failed'
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
    if (locked.value || !registered.value) return

    const previous = [...selectedIds.value]
    const next = isSelected(heroId)
      ? previous.filter((id) => id !== heroId)
      : [...previous, heroId]

    if (next.length > rosterSize.value) {
      error.value = `Roster holds ${rosterSize.value} heroes. Drop one first.`
      return
    }

    selectedIds.value = next
    error.value = null
    violations.value = []
    saving.value = true
    try {
      adopt(await api.setSlots(tournamentId.value!, next))
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
    isSelected,
    select,
    load,
    register,
    toggle,
    lock,
  }
})
