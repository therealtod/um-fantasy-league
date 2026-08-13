<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { api, describeError } from '@/api/client'
import { useTournamentsStore } from '@/stores/tournaments'
import { byName } from '@/lib/sort'
import type { MapAdminDto } from '@/api/types'
import ErrorBanner from '@/components/ErrorBanner.vue'
import DestructiveConfirmPanel from '@/components/DestructiveConfirmPanel.vue'
import TournamentSelect from '@/components/TournamentSelect.vue'

const tournamentsStore = useTournamentsStore()

const selectedTournamentId = ref<number | null>(null)
const maps = ref<MapAdminDto[]>([])
const mapPool = ref<MapAdminDto[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const removingMap = ref<MapAdminDto | null>(null)

// Maps staged for the next batch submit — checked here, sent in one request.
const selectedMapIds = ref<number[]>([])

const tournaments = computed(() => tournamentsStore.tournaments)

// Maps not yet in this tournament's pool — the only ones worth adding.
const availableMaps = computed(() => {
  const poolIds = new Set(mapPool.value.map((m) => m.id))
  return maps.value.filter((m) => !poolIds.has(m.id))
})

onMounted(() => {
  void loadMaps()
})

async function loadMaps() {
  try {
    maps.value = (await api.admin.listMaps()).sort(byName)
  } catch (e) {
    error.value = describeError(e, 'Failed to load maps')
  }
}

async function loadMapPool(tournamentId: number) {
  try {
    const pool = await api.admin.listMapPool(tournamentId)
    // A later switch may have already changed the selection while this was in flight — drop it.
    if (selectedTournamentId.value !== tournamentId) return
    mapPool.value = pool
  } catch (e) {
    if (selectedTournamentId.value !== tournamentId) return
    error.value = describeError(e, 'Failed to load map pool')
  }
}

// Clear selection and reload the pool when tournament changes
watch(selectedTournamentId, async (newId) => {
  selectedMapIds.value = []
  removingMap.value = null
  if (newId !== null) {
    await loadMapPool(newId)
  } else {
    mapPool.value = []
  }
})

async function submitBatch() {
  if (!selectedTournamentId.value || selectedMapIds.value.length === 0) return

  loading.value = true
  error.value = null

  try {
    await api.admin.addMapsToPool(selectedTournamentId.value, selectedMapIds.value)
    selectedMapIds.value = []
    await loadMapPool(selectedTournamentId.value)
  } catch (e) {
    error.value = describeError(e, 'Failed to add maps to pool')
  } finally {
    loading.value = false
  }
}

function startRemoveMap(map: MapAdminDto) {
  error.value = null
  removingMap.value = map
}

function cancelRemoveMap() {
  removingMap.value = null
}

async function confirmRemoveMap() {
  if (!selectedTournamentId.value || !removingMap.value) return

  loading.value = true
  error.value = null

  try {
    await api.admin.removeMapFromPool(selectedTournamentId.value, removingMap.value.id)
    removingMap.value = null
    await loadMapPool(selectedTournamentId.value)
  } catch (e) {
    // A 409 here means the tournament already has a match recorded on this board.
    error.value = describeError(e, 'Failed to remove map from pool')
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">Map Pool Management</h2>
    </div>

    <ErrorBanner :message="error" />

    <!-- Tournament selector -->
    <TournamentSelect v-model="selectedTournamentId" :tournaments="tournaments" />

    <!-- Add maps form — check several, then submit once as a batch -->
    <div v-if="selectedTournamentId && !removingMap" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">Add Maps to Tournament Pool</h3>

      <p v-if="maps.length === 0" class="font-mono text-xs text-ink-dim italic">
        No maps exist yet. Create one in the Maps section first.
      </p>
      <p v-else-if="availableMaps.length === 0" class="font-mono text-xs text-ink-dim italic">
        Every map is already in this tournament's pool.
      </p>
      <div v-else class="flex flex-col gap-2">
        <label
          v-for="map in availableMaps"
          :key="map.id"
          class="flex cursor-pointer items-center gap-3 border border-edge bg-surface-lowest px-3 py-2 font-mono text-sm text-ink"
        >
          <input v-model="selectedMapIds" type="checkbox" :value="map.id" class="cursor-pointer" />
          {{ map.name }}
        </label>
      </div>

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-primary" :disabled="loading || selectedMapIds.length === 0" @click="submitBatch">
          {{
            loading
              ? 'Adding...'
              : `Add ${selectedMapIds.length} ${selectedMapIds.length === 1 ? 'Map' : 'Maps'} to Pool`
          }}
        </button>
      </div>
    </div>

    <!-- Remove confirmation — the API rejects this if a match was already played on the board -->
    <DestructiveConfirmPanel
      v-if="removingMap"
      title="Remove Map from Pool"
      confirm-label="Remove from Pool"
      busy-label="Removing..."
      :busy="loading"
      @cancel="cancelRemoveMap"
      @confirm="confirmRemoveMap"
    >
      Remove <strong>{{ removingMap.name }}</strong> from this tournament's pool? This is rejected
      if the tournament already has a match recorded on this board.
    </DestructiveConfirmPanel>

    <!-- Current pool -->
    <div v-if="selectedTournamentId && !removingMap" class="flex flex-col gap-3">
      <h3 class="headline text-lg text-cyan">Current Pool</h3>
      <div v-if="mapPool.length === 0" class="p-12 text-center">
        <p class="text-ink-dim">No maps in this tournament's pool yet.</p>
      </div>
      <ul v-else class="flex flex-col gap-2">
        <li
          v-for="map in mapPool"
          :key="map.id"
          class="panel flex flex-wrap items-center justify-between gap-3 p-4"
        >
          <span class="headline text-base text-ink">{{ map.name }}</span>
          <button
            class="border border-magenta px-4 py-1.5 font-mono text-xs text-magenta transition-opacity hover:opacity-85"
            @click="startRemoveMap(map)"
          >
            Remove
          </button>
        </li>
      </ul>
    </div>
  </div>
</template>
