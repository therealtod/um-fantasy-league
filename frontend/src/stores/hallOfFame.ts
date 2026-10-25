import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '@/api/client'
import { useAsyncRequest } from '@/composables/useAsyncRequest'
import type { HallOfFame } from '@/api/types'

/**
 * The completed tournaments' podiums and the all-time titles table.
 *
 * Unlike `standings` there is no stream: a completed tournament's results
 * rarely move, and the view reloads every time it is opened, so a correction
 * shows up on the next visit.
 */
export const useHallOfFameStore = defineStore('hallOfFame', () => {
  const hallOfFame = ref<HallOfFame | null>(null)
  const { loading, error, run } = useAsyncRequest()

  async function load() {
    const result = await run(() => api.hallOfFame(), 'Could not load the Hall of Fame')
    if (result.ok) hallOfFame.value = result.value
  }

  return { hallOfFame, loading, error, load }
})
