import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '@/api/client'
import { useAsyncRequest } from '@/composables/useAsyncRequest'
import type { Manager } from '@/api/types'

export const useManagerStore = defineStore('manager', () => {
  const manager = ref<Manager | null>(null)
  const { loading, error, run } = useAsyncRequest()

  async function load() {
    const result = await run(() => api.me(), 'Could not load manager')
    if (result.ok) manager.value = result.value
  }

  /** Drops the loaded identity. Called on sign-out so the header stops showing the previous manager. */
  function reset() {
    manager.value = null
    error.value = null
  }

  return { manager, loading, error, load, reset }
})
