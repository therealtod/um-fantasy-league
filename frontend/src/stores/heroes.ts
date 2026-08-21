import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '@/api/client'
import { useAsyncRequest } from '@/composables/useAsyncRequest'
import type { Hero, HeroSort } from '@/api/types'

/**
 * The hero pool for one tournament. Cost is tournament-scoped, so the pool is
 * keyed by tournament and reloaded whenever that changes.
 */
export const useHeroesStore = defineStore('heroes', () => {
  const heroes = ref<Hero[]>([])
  const tournamentId = ref<number | null>(null)
  const { loading, error, run } = useAsyncRequest()

  const sort = ref<HeroSort>('COST')
  const search = ref('')

  async function load(id: number | null = tournamentId.value) {
    if (id === null) {
      tournamentId.value = null
      heroes.value = []
      return
    }
    tournamentId.value = id
    // run() drops an out-of-order response on its own — a search request
    // that started earlier but resolves later than a newer one is dropped
    // instead of clobbering it.
    const result = await run(
      () => api.heroes(id, { sort: sort.value, search: search.value || undefined }),
      'Could not load heroes',
    )
    if (result.ok) heroes.value = result.value
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
