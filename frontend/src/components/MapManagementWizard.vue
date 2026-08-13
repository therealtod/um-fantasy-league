<script setup lang="ts">
import { api } from '@/api/client'
import type { MapAdminDto, CreateMapRequest } from '@/api/types'
import SimpleCrudWizard from '@/components/SimpleCrudWizard.vue'

function emptyForm(): CreateMapRequest {
  return { name: '' }
}
</script>

<template>
  <SimpleCrudWizard
    entity-label="Map"
    empty-message="No maps loaded. Create a new map to begin."
    :load="api.admin.listMaps"
    :create="api.admin.createMap"
    :update="api.admin.updateMap"
    :empty-form="emptyForm"
    :to-form="(map: MapAdminDto): CreateMapRequest => ({ name: map.name })"
  >
    <template #fields="{ form }">
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
    </template>

    <template #item="{ item }">
      <span class="headline text-base text-ink">{{ item.name }}</span>
    </template>
  </SimpleCrudWizard>
</template>
