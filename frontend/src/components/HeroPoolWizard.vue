<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { api } from '@/api/client'
import { useTournamentsStore } from '@/stores/tournaments'
import type { Hero, HeroAdminDto } from '@/api/types'

const tournamentsStore = useTournamentsStore()

const selectedTournamentId = ref<number | null>(null)
const heroes = ref<HeroAdminDto[]>([])
const heroPoolHeroes = ref<Hero[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const showAddForm = ref(false)

const addForm = ref({
  heroId: null as number | null,
  cost: 1000,
})

const tournaments = computed(() => tournamentsStore.tournaments)

// Heroes not yet in this tournament's pool — the only ones worth adding.
const availableHeroes = computed(() => {
  const poolIds = new Set(heroPoolHeroes.value.map((h) => h.id))
  return heroes.value.filter((h) => !poolIds.has(h.id))
})

onMounted(() => {
  void loadHeroes()
})

async function loadHeroes() {
  try {
    heroes.value = await api.admin.listHeroes()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load heroes'
  }
}

// Watch tournament selection to load its hero pool
watch(selectedTournamentId, async (newId) => {
  if (newId !== null) {
    await loadHeroPool(newId)
  } else {
    heroPoolHeroes.value = []
  }
})

async function loadHeroPool(tournamentId: number) {
  loading.value = true
  error.value = null
  try {
    heroPoolHeroes.value = await api.heroes(tournamentId)
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load hero pool'
  } finally {
    loading.value = false
  }
}

function startAddHero() {
  addForm.value = { heroId: null, cost: 1000 }
  showAddForm.value = true
}

function cancelAddForm() {
  showAddForm.value = false
  addForm.value = { heroId: null, cost: 1000 }
}

async function addHeroToPool() {
  if (!selectedTournamentId.value || !addForm.value.heroId) {
    error.value = 'Please select a hero and enter a cost'
    return
  }

  loading.value = true
  error.value = null

  try {
    await api.admin.setHeroCost(
      selectedTournamentId.value,
      addForm.value.heroId,
      addForm.value.cost,
    )
    await loadHeroPool(selectedTournamentId.value)
    cancelAddForm()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to add hero to pool'
  } finally {
    loading.value = false
  }
}

async function updateHeroCost(heroId: number, newCost: number) {
  if (!selectedTournamentId.value) return

  loading.value = true
  error.value = null

  try {
    await api.admin.setHeroCost(selectedTournamentId.value, heroId, newCost)
    await loadHeroPool(selectedTournamentId.value)
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to update hero cost'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">Hero Pool & Pricing</h2>
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

    <!-- Hero pool controls -->
    <div v-if="selectedTournamentId && !showAddForm" class="flex gap-3">
      <button class="btn-primary" @click="startAddHero">+ Add Hero to Pool</button>
    </div>

    <!-- Add hero form -->
    <div v-if="showAddForm" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">Add Hero to Pool</h3>

      <div class="flex flex-col gap-2">
        <label for="hero-select" class="label-caps">Hero *</label>
        <select
          id="hero-select"
          v-model.number="addForm.heroId"
          class="cursor-pointer border border-edge bg-surface-lowest px-3 py-2 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
        >
          <option :value="null">-- Choose a hero --</option>
          <option v-for="hero in availableHeroes" :key="hero.id" :value="hero.id">
            {{ hero.name }}
          </option>
        </select>
        <p v-if="heroes.length === 0" class="font-mono text-xs text-ink-dim italic">
          No heroes exist yet. Create one in the Heroes section first.
        </p>
        <p v-else-if="availableHeroes.length === 0" class="font-mono text-xs text-ink-dim italic">
          Every hero is already in this tournament's pool.
        </p>
      </div>

      <div class="flex flex-col gap-2">
        <label for="hero-cost-input" class="label-caps">Cost (Credits) *</label>
        <input
          id="hero-cost-input"
          v-model.number="addForm.cost"
          type="number"
          min="1"
          class="border border-edge bg-surface-lowest px-3 py-2 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
          placeholder="e.g., 5000"
        />
      </div>

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="cancelAddForm">Cancel</button>
        <button class="btn-primary" :disabled="loading || !addForm.heroId" @click="addHeroToPool">
          {{ loading ? 'Adding...' : 'Add to Pool' }}
        </button>
      </div>
    </div>

    <!-- Hero pool list -->
    <div v-if="selectedTournamentId && !showAddForm" class="flex flex-col gap-2">
      <div v-if="heroPoolHeroes.length === 0" class="p-12 text-center">
        <p class="text-ink-dim">No heroes in this tournament's pool yet. Add heroes to begin.</p>
      </div>

      <div
        v-for="hero in heroPoolHeroes"
        :key="hero.id"
        class="panel flex flex-wrap items-center justify-between gap-3 p-4"
      >
        <div class="flex flex-col gap-1">
          <span class="headline text-base text-ink">{{ hero.name }}</span>
          <span class="font-mono text-xs text-ink-dim">ID: {{ hero.id }}</span>
        </div>
        <div class="flex items-center gap-2">
          <label :for="`cost-${hero.id}`" class="label-caps">Cost:</label>
          <input
            :id="`cost-${hero.id}`"
            :value="hero.cost"
            type="number"
            min="1"
            class="w-24 border border-edge bg-surface-lowest px-2 py-1.5 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
            @change="
              (e) => updateHeroCost(hero.id, Number((e.target as HTMLInputElement).value))
            "
          />
          <span class="font-mono text-xs text-ink-dim">CR</span>
        </div>
      </div>
    </div>
  </div>
</template>
