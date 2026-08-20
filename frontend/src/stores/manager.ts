import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api, describeError } from '@/api/client'
import type { Manager } from '@/api/types'

export const useManagerStore = defineStore('manager', () => {
  const manager = ref<Manager | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function load() {
    loading.value = true
    error.value = null
    try {
      manager.value = await api.me()
    } catch (e) {
      error.value = describeError(e, 'Could not load manager')
    } finally {
      loading.value = false
    }
  }

  /** Drops the loaded identity. Called on sign-out so the header stops showing the previous manager. */
  function reset() {
    manager.value = null
    error.value = null
  }

  return { manager, loading, error, load, reset }
})
