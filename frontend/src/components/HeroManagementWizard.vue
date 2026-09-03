<script setup lang="ts">
import { api } from '@/api/client'
import type { HeroAdminDto, CreateHeroRequest } from '@/api/types'
import SimpleCrudWizard from '@/components/SimpleCrudWizard.vue'

function emptyForm(): CreateHeroRequest {
  return { name: '', imageUrl: null }
}
</script>

<template>
  <SimpleCrudWizard
    entity-label="Hero"
    empty-message="No heroes loaded. Create a new hero or load heroes from a tournament's pool."
    :load="api.admin.listHeroes"
    :create="api.admin.createHero"
    :update="api.admin.updateHero"
    :empty-form="emptyForm"
    :to-form="(hero: HeroAdminDto): CreateHeroRequest => ({ name: hero.name, imageUrl: hero.imageUrl })"
  >
    <template #fields="{ form }">
      <div class="flex flex-col gap-2">
        <label for="hero-name" class="label-caps">Name *</label>
        <input
          id="hero-name"
          v-model="form.name"
          type="text"
          class="field-input"
          placeholder="e.g., Alice, King Arthur, Medusa"
        />
      </div>

      <div class="flex flex-col gap-2">
        <label for="hero-image" class="label-caps">Image URL (optional)</label>
        <input
          id="hero-image"
          v-model="form.imageUrl"
          type="text"
          class="field-input"
          placeholder="https://..."
        />
      </div>
    </template>

    <template #item="{ item }">
      <div class="flex min-w-0 flex-col gap-1">
        <span class="headline text-base text-ink">{{ item.name }}</span>
        <span v-if="item.imageUrl" class="break-all font-mono text-xs text-ink-dim">{{ item.imageUrl }}</span>
        <span v-else class="font-mono text-xs text-ink-dim italic">No image</span>
      </div>
    </template>
  </SimpleCrudWizard>
</template>
