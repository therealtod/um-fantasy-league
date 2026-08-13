<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { api } from '@/api/client'
import type { HeroAdminDto, CreateHeroRequest } from '@/api/types'
import ErrorBanner from '@/components/ErrorBanner.vue'

const heroes = ref<HeroAdminDto[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const showForm = ref(false)
const editingHero = ref<HeroAdminDto | null>(null)

const form = ref<CreateHeroRequest>({
  name: '',
  imageUrl: null,
})

onMounted(() => {
  void loadHeroes()
})

async function loadHeroes() {
  loading.value = true
  error.value = null
  try {
    heroes.value = await api.admin.listHeroes()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load heroes'
  } finally {
    loading.value = false
  }
}

function startCreate() {
  editingHero.value = null
  form.value = { name: '', imageUrl: null }
  showForm.value = true
}

function startEdit(hero: HeroAdminDto) {
  editingHero.value = hero
  form.value = { name: hero.name, imageUrl: hero.imageUrl }
  showForm.value = true
}

function cancelForm() {
  showForm.value = false
  editingHero.value = null
  form.value = { name: '', imageUrl: null }
}

async function saveHero() {
  if (!form.value.name.trim()) {
    error.value = 'Hero name is required'
    return
  }

  loading.value = true
  error.value = null

  try {
    if (editingHero.value) {
      const updated = await api.admin.updateHero(editingHero.value.id, form.value)
      const index = heroes.value.findIndex((h) => h.id === updated.id)
      if (index !== -1) {
        heroes.value[index] = updated
      }
    } else {
      const created = await api.admin.createHero(form.value)
      heroes.value.push(created)
    }
    cancelForm()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to save hero'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">Hero Management</h2>
      <button v-if="!showForm" class="btn-primary" @click="startCreate">+ Create Hero</button>
    </div>

    <ErrorBanner :message="error" />

    <!-- Form -->
    <div v-if="showForm" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">{{ editingHero ? 'Edit Hero' : 'Create New Hero' }}</h3>

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

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="cancelForm">Cancel</button>
        <button class="btn-primary" :disabled="loading" @click="saveHero">
          {{ loading ? 'Saving...' : 'Save Hero' }}
        </button>
      </div>
    </div>

    <!-- Heroes list -->
    <div v-if="!showForm" class="flex flex-col gap-2">
      <div v-if="heroes.length === 0" class="p-12 text-center">
        <p class="text-ink-dim">
          No heroes loaded. Create a new hero or load heroes from a tournament's pool.
        </p>
      </div>

      <div
        v-for="hero in heroes"
        :key="hero.id"
        class="panel flex flex-wrap items-center justify-between gap-3 p-4"
      >
        <div class="flex flex-col gap-1">
          <span class="headline text-base text-ink">{{ hero.name }}</span>
          <span v-if="hero.imageUrl" class="font-mono text-xs text-ink-dim">{{ hero.imageUrl }}</span>
          <span v-else class="font-mono text-xs text-ink-dim italic">No image</span>
        </div>
        <button class="btn-ghost px-4 py-2 text-xs" @click="startEdit(hero)">Edit</button>
      </div>
    </div>
  </div>
</template>
