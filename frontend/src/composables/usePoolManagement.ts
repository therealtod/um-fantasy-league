import { computed, onMounted, ref, watch } from 'vue'
import { useAsyncRequest } from '@/composables/useAsyncRequest'
import { useTournamentsStore } from '@/stores/tournaments'

/**
 * The state and flow `HeroPoolWizard` and `MapPoolWizard` share: pick a
 * tournament, load its catalog and its current pool (dropping a pool load
 * superseded by a tournament switch, on top of `useAsyncRequest`'s own
 * out-of-order guard — a switch can outrace the load it started), and remove
 * an item from the pool behind a confirm step.
 *
 * What differs between a hero pool (a staged batch with a per-hero cost) and
 * a map pool (a flat checkbox multi-select, no cost) is the *add* flow, which
 * deliberately stays in each component rather than becoming slots on a
 * wrapper component: that flow needs script-level access to `run`/`error`/
 * `reloadPool` (a hero's `stageHero` writes `error` directly; a cost edit
 * calls `run` on its own), and a scoped slot only hands those to the
 * *template*, not to the plain functions a `<script setup>` block defines. A
 * composable hands back live refs and functions either can use, so the piece
 * that was never actually duplicated — how each screen builds its batch —
 * is free to stay exactly as different as it already was.
 */
export function usePoolManagement<TCatalog, TPool extends { id: number }>(config: {
  entityLabel: string
  loadCatalog: () => Promise<TCatalog[]>
  loadPool: (tournamentId: number) => Promise<TPool[]>
  removeFromPool: (tournamentId: number, itemId: number) => Promise<void>
}) {
  const tournamentsStore = useTournamentsStore()
  const tournaments = computed(() => tournamentsStore.tournaments)

  const selectedTournamentId = ref<number | null>(null)
  const catalog = ref<TCatalog[]>([])
  const pool = ref<TPool[]>([])
  const { loading, error, violations, run } = useAsyncRequest()
  const removingItem = ref<TPool | null>(null)

  const label = config.entityLabel.toLowerCase()

  onMounted(() => {
    void loadCatalog()
  })

  async function loadCatalog() {
    const result = await run(() => config.loadCatalog(), `Failed to load ${label}s`)
    if (result.ok) catalog.value = result.value
  }

  async function reloadPool(tournamentId: number) {
    const result = await run(() => config.loadPool(tournamentId), `Failed to load ${label} pool`)
    // A later switch may have already changed the selection while this was
    // in flight — run() already drops that out-of-order response on its own.
    if (selectedTournamentId.value !== tournamentId) return
    if (result.ok) pool.value = result.value
  }

  watch(selectedTournamentId, async (newId) => {
    removingItem.value = null
    if (newId !== null) {
      await reloadPool(newId)
    } else {
      pool.value = []
    }
  })

  function startRemove(item: TPool) {
    error.value = null
    violations.value = []
    removingItem.value = item
  }

  function cancelRemove() {
    removingItem.value = null
  }

  async function confirmRemove() {
    if (!selectedTournamentId.value || !removingItem.value) return
    const tournamentId = selectedTournamentId.value
    const itemId = removingItem.value.id

    const result = await run(
      () => config.removeFromPool(tournamentId, itemId),
      `Failed to remove ${label} from pool`,
    )
    if (!result.ok) return
    removingItem.value = null
    await reloadPool(tournamentId)
  }

  return {
    tournaments,
    selectedTournamentId,
    catalog,
    pool,
    loading,
    error,
    violations,
    run,
    reloadPool,
    removingItem,
    startRemove,
    cancelRemove,
    confirmRemove,
  }
}
