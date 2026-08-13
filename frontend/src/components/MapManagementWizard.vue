<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { api } from '@/api/client'
import type { MapAdminDto, CreateMapRequest } from '@/api/types'

const maps = ref<MapAdminDto[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const showForm = ref(false)
const editingMap = ref<MapAdminDto | null>(null)

const form = ref<CreateMapRequest>({
  name: '',
})

onMounted(() => {
  void loadMaps()
})

async function loadMaps() {
  loading.value = true
  error.value = null
  try {
    maps.value = await api.admin.listMaps()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load maps'
  } finally {
    loading.value = false
  }
}

function startCreate() {
  editingMap.value = null
  form.value = { name: '' }
  showForm.value = true
}

function startEdit(map: MapAdminDto) {
  editingMap.value = map
  form.value = { name: map.name }
  showForm.value = true
}

function cancelForm() {
  showForm.value = false
  editingMap.value = null
  form.value = { name: '' }
}

async function saveMap() {
  if (!form.value.name.trim()) {
    error.value = 'Map name is required'
    return
  }

  loading.value = true
  error.value = null

  try {
    if (editingMap.value) {
      const updated = await api.admin.updateMap(editingMap.value.id, form.value)
      const index = maps.value.findIndex((m) => m.id === updated.id)
      if (index !== -1) {
        maps.value[index] = updated
      }
    } else {
      const created = await api.admin.createMap(form.value)
      maps.value.push(created)
    }
    cancelForm()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to save map'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">Map Management</h2>
      <button v-if="!showForm" class="btn-primary" @click="startCreate">+ Create Map</button>
    </div>

    <p v-if="error" class="border border-magenta/50 bg-magenta/10 p-4 font-mono text-sm text-magenta">
      {{ error }}
    </p>

    <!-- Form -->
    <div v-if="showForm" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">{{ editingMap ? 'Edit Map' : 'Create New Map' }}</h3>

      <div class="flex flex-col gap-2">
        <label for="map-name" class="label-caps">Name *</label>
        <input
          id="map-name"
          v-model="form.name"
          type="text"
          class="field-input"
          placeholder="e.g., Baskerville Manor, Sherwood Forest"
        />
      </div>

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="cancelForm">Cancel</button>
        <button class="btn-primary" :disabled="loading" @click="saveMap">
          {{ loading ? 'Saving...' : 'Save Map' }}
        </button>
      </div>
    </div>

    <!-- Maps list -->
    <div v-if="!showForm" class="flex flex-col gap-2">
      <div v-if="maps.length === 0" class="p-12 text-center">
        <p class="text-ink-dim">No maps loaded. Create a new map to begin.</p>
      </div>

      <div
        v-for="map in maps"
        :key="map.id"
        class="panel flex flex-wrap items-center justify-between gap-3 p-4"
      >
        <span class="headline text-base text-ink">{{ map.name }}</span>
        <button class="btn-ghost px-4 py-2 text-xs" @click="startEdit(map)">Edit</button>
      </div>
    </div>
  </div>
</template>
