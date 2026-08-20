import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { api, describeError } from '@/api/client'
import { openStandingsStream } from '@/api/sseClient'
import type { StandingsBoard, TickerEntry } from '@/api/types'

const MAX_TICKER_ENTRIES = 40

export const useStandingsStore = defineStore('standings', () => {
  const tournamentId = ref<number | null>(null)
  const board = ref<StandingsBoard | null>(null)
  const ticker = ref<TickerEntry[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  /** Closes the current tournament's `/standings/stream` SSE connection, if any. */
  let closeStream: (() => void) | null = null

  const rows = computed(() => board.value?.rows ?? [])
  const metrics = computed(() => board.value?.metrics ?? [])

  async function load(id: number) {
    closeStream?.()
    closeStream = null

    tournamentId.value = id
    board.value = null
    ticker.value = []
    loading.value = true
    error.value = null
    try {
      const [standings, matches] = await Promise.all([
        api.standings(id),
        api.matches(id, 0, MAX_TICKER_ENTRIES),
      ])
      // A second load() for a different tournament may have started and even
      // finished while this one was still in flight — an out-of-order
      // response here must not clobber the newer selection's board.
      if (tournamentId.value !== id) return
      board.value = standings
      ticker.value = matches
    } catch (e) {
      if (tournamentId.value !== id) return
      error.value = describeError(e, 'Could not load standings')
    } finally {
      if (tournamentId.value === id) loading.value = false
    }

    if (tournamentId.value === id) {
      closeStream = openStandingsStream(id, () => void refresh())
    }
  }

  /** Closes the live standings stream, if one is open. Call on unmount. */
  function stop() {
    closeStream?.()
    closeStream = null
  }

  /**
   * Refresh the leaderboard and ticker unconditionally. A correction reuses an
   * existing match id and a deletion removes one, so an incremental "since"
   * fetch can miss both — only a full refetch of the ticker head reflects
   * either.
   */
  async function refresh() {
    const id = tournamentId.value
    if (id === null) return
    try {
      const [standings, matches] = await Promise.all([
        api.standings(id),
        api.matches(id, 0, MAX_TICKER_ENTRIES),
      ])
      // A tournament switch (or another refresh) may have landed while this
      // one was in flight — an out-of-order response must not clobber the
      // newer selection's board.
      if (tournamentId.value !== id) return
      board.value = standings
      ticker.value = matches
    } catch {
      // A transient refresh failure should not tear down the screen; the data
      // already on the page stays valid.
    }
  }

  return {
    tournamentId,
    board,
    rows,
    metrics,
    ticker,
    loading,
    error,
    load,
    refresh,
    stop,
  }
})
