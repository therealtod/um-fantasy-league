import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { api, ApiError } from '@/api/client'
import type { Tournament } from '@/api/types'

export const useTournamentsStore = defineStore('tournaments', () => {
  const tournaments = ref<Tournament[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  const registering = ref<number | null>(null)

  // `myEntryStatus` is *absent*, not null, when the manager has no entry: the
  // backend serialises with `default-property-inclusion: non_null`. Test it for
  // truthiness rather than against null, or every one of these reads inverts.

  /** Tournaments the manager can currently draft a roster for. */
  const draftable = computed(() =>
    tournaments.value.filter((t) => Boolean(t.myEntryStatus) || t.acceptsRegistration),
  )

  /** Tournaments this manager has not yet joined and can still enter. */
  const openForRegistration = computed(() =>
    tournaments.value.filter((t) => !t.myEntryStatus && t.acceptsRegistration),
  )

  const live = computed(() => tournaments.value.filter((t) => t.status === 'LIVE'))

  async function load() {
    loading.value = true
    error.value = null
    try {
      tournaments.value = await api.tournaments()
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Could not load tournaments'
    } finally {
      loading.value = false
    }
  }

  async function register(tournamentId: number) {
    registering.value = tournamentId
    error.value = null
    try {
      await api.register(tournamentId)
      // Enrolment count and entry status both move — refetch.
      await load()
      return true
    } catch (e) {
      error.value = e instanceof ApiError ? e.message : 'Registration failed'
      return false
    } finally {
      registering.value = null
    }
  }

  function byId(id: number) {
    return tournaments.value.find((t) => t.id === id) ?? null
  }

  return {
    tournaments,
    loading,
    error,
    registering,
    draftable,
    openForRegistration,
    live,
    load,
    register,
    byId,
  }
})
