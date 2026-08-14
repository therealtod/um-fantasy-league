import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api, describeError } from '@/api/client'
import type { Hero, HeroSort } from '@/api/types'

/**
 * The hero pool for one tournament. Cost is tournament-scoped, so the pool is
 * keyed by tournament and reloaded whenever that changes.
 */
export const useHeroesStore = defineStore('heroes', () => {
  const heroes = ref<Hero[]>([])
  const tournamentId = ref<number | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  const sort = ref<HeroSort>('COST')
  const search = ref('')

  // Bumped on every load() so an out-of-order response — a search request
  // that started earlier but resolves later than a newer one — can be told
  // apart from the latest and dropped instead of clobbering it.
  let requestId = 0

  async function load(id: number | null = tournamentId.value) {
    if (id === null) {
      tournamentId.value = null
      heroes.value = []
      return
    }
    tournamentId.value = id
    const thisRequest = ++requestId
    loading.value = true
    error.value = null
    try {
      const result = await api.heroes(id, {
        sort: sort.value,
        search: search.value || undefined,
      })
      if (thisRequest !== requestId) return
      heroes.value = result
    } catch (e) {
      if (thisRequest !== requestId) return
      error.value = describeError(e, 'Could not load heroes')
    } finally {
      if (thisRequest === requestId) loading.value = false
    }
  }

  /** Point the pool at a different tournament, reloading only when it moves. */
  async function select(id: number | null) {
    if (id === tournamentId.value) return
    heroes.value = []
    await load(id)
  }

  function byId(id: number) {
    return heroes.value.find((hero) => hero.id === id) ?? null
  }

  return { heroes, tournamentId, loading, error, sort, search, load, select, byId }
})
