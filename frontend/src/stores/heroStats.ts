import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '@/api/client'
import { openStandingsStream } from '@/api/sseClient'
import { useAsyncRequest } from '@/composables/useAsyncRequest'
import type { HeroStatsBoard } from '@/api/types'

/**
 * Per-hero rankings for one tournament. Shaped like the `standings` store and
 * driven by the same `/standings/stream` push: a recorded match changes the
 * hero tables exactly when it changes the leaderboard, so there is no second
 * stream to keep in step.
 */
export const useHeroStatsStore = defineStore('heroStats', () => {
  const tournamentId = ref<number | null>(null)
  const board = ref<HeroStatsBoard | null>(null)
  const { loading, error, run } = useAsyncRequest()

  let closeStream: (() => void) | null = null

  async function load(id: number) {
    closeStream?.()
    closeStream = null

    tournamentId.value = id
    board.value = null

    const result = await run(() => api.heroStats(id), 'Could not load hero performance')
    // A later load() for another tournament owns the stream now.
    if (tournamentId.value !== id) return
    if (result.ok) board.value = result.value
    closeStream = openStandingsStream(id, () => void refresh())
  }

  /** Closes the live stream, if one is open. Call on unmount. */
  function stop() {
    closeStream?.()
    closeStream = null
  }

  /** Refetch after a match write; a failure keeps the tables already shown. */
  async function refresh() {
    const id = tournamentId.value
    if (id === null) return
    try {
      const fresh = await api.heroStats(id)
      if (tournamentId.value !== id) return
      board.value = fresh
    } catch {
      // Transient: the data on the page is still valid.
    }
  }

  return { tournamentId, board, loading, error, load, refresh, stop }
})
