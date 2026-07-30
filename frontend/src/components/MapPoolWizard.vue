<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { api } from '@/api/client'
import { useTournamentsStore } from '@/stores/tournaments'
import type { MapAdminDto } from '@/api/types'

const tournamentsStore = useTournamentsStore()

const selectedTournamentId = ref<number | null>(null)
const maps = ref<MapAdminDto[]>([])
const mapPool = ref<MapAdminDto[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

const addForm = ref({
  mapId: null as number | null,
})

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
    maps.value = await api.admin.listMaps()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load maps'
  }
}

async function loadMapPool(tournamentId: number) {
  try {
    mapPool.value = await api.admin.listMapPool(tournamentId)
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load map pool'
  }
}

// Clear form and reload the pool when tournament changes
watch(selectedTournamentId, async (newId) => {
  addForm.value = { mapId: null }
  if (newId !== null) {
    await loadMapPool(newId)
  } else {
    mapPool.value = []
  }
})

async function addMapToPool() {
  if (!selectedTournamentId.value || !addForm.value.mapId) {
    error.value = 'Please select a map'
    return
  }

  loading.value = true
  error.value = null

  try {
    await api.admin.addMapToPool(selectedTournamentId.value, addForm.value.mapId)
    addForm.value = { mapId: null }
    await loadMapPool(selectedTournamentId.value)
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to add map to pool'
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

    <p v-if="error" class="border border-magenta/50 bg-magenta/10 p-4 font-mono text-sm text-magenta">
      {{ error }}
    </p>

    <!-- Tournament selector -->
    <div class="flex flex-col gap-2">
      <label for="tournament-select" class="label-caps">Select Tournament *</label>
      <select
        id="tournament-select"
        v-model.number="selectedTournamentId"
        class="cursor-pointer border border-edge bg-surface-lowest px-3 py-2 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
      >
        <option :value="null">-- Choose a tournament --</option>
        <option v-for="tournament in tournaments" :key="tournament.id" :value="tournament.id">
          {{ tournament.name }}
        </option>
      </select>
    </div>

    <!-- Add map form -->
    <div v-if="selectedTournamentId" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">Add Map to Tournament Pool</h3>

      <div class="flex flex-col gap-2">
        <label for="map-select" class="label-caps">Map *</label>
        <select
          id="map-select"
          v-model.number="addForm.mapId"
          class="cursor-pointer border border-edge bg-surface-lowest px-3 py-2 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
        >
          <option :value="null">-- Choose a map --</option>
          <option v-for="map in availableMaps" :key="map.id" :value="map.id">
            {{ map.name }}
          </option>
        </select>
        <p v-if="maps.length === 0" class="font-mono text-xs text-ink-dim italic">
          No maps exist yet. Create one in the Maps section first.
        </p>
        <p v-else-if="availableMaps.length === 0" class="font-mono text-xs text-ink-dim italic">
          Every map is already in this tournament's pool.
        </p>
      </div>

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-primary" :disabled="loading || !addForm.mapId" @click="addMapToPool">
          {{ loading ? 'Adding...' : 'Add to Pool' }}
        </button>
      </div>
    </div>

    <!-- Current pool -->
    <div v-if="selectedTournamentId" class="flex flex-col gap-3">
      <h3 class="headline text-lg text-cyan">Current Pool</h3>
      <div v-if="mapPool.length === 0" class="p-12 text-center">
        <p class="text-ink-dim">No maps in this tournament's pool yet.</p>
      </div>
      <ul v-else class="flex flex-col gap-2">
        <li
          v-for="map in mapPool"
          :key="map.id"
          class="panel headline p-4 text-base text-ink"
        >
          {{ map.name }}
        </li>
      </ul>
    </div>
  </div>
</template>
