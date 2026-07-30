import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '@/api/client'
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
      error.value = e instanceof Error ? e.message : 'Could not load manager'
    } finally {
      loading.value = false
    }
  }

  return { manager, loading, error, load }
})
