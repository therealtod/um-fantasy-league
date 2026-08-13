<script setup lang="ts">
import { computed, onMounted, watch } from 'vue'
import { useRoute } from 'vue-router'
import BudgetMeter from '@/components/BudgetMeter.vue'
import HeroCard from '@/components/HeroCard.vue'
import RosterUplinkPanel from '@/components/RosterUplinkPanel.vue'
import { useHeroesStore } from '@/stores/heroes'
import { useRosterStore } from '@/stores/roster'
import type { HeroSort } from '@/api/types'

const route = useRoute()
const heroesStore = useHeroesStore()
const rosterStore = useRosterStore()

const SORTS: { value: HeroSort; label: string }[] = [
  { value: 'COST', label: 'Cost' },
  { value: 'NAME', label: 'Name' },
]

const tournamentId = computed(() => Number(route.params.tournamentId))

onMounted(async () => {
  await heroesStore.select(tournamentId.value)
  if (rosterStore.tournamentId !== tournamentId.value) {
    await rosterStore.select(tournamentId.value)
  }
})

// Cost is tournament-scoped, so a route change re-fetches the whole pool.
watch(tournamentId, (id) => void heroesStore.select(id))

watch(
  () => [heroesStore.sort, heroesStore.search],
  () => void heroesStore.load(),
)
</script>

<template>
  <div class="flex flex-col gap-6 lg:flex-row">
    <!-- Hero pool -->
    <section class="min-w-0 flex-1">
      <div class="panel sticky top-0 z-20 mb-4 p-3 lg:hidden">
        <BudgetMeter :budget="rosterStore.budget" />
      </div>

      <!-- Entry state -->
      <div class="panel flex flex-wrap items-center justify-between gap-4 p-4">
        <div class="flex items-center gap-3">
          <span class="label-caps">Deployment</span>
          <span class="font-mono text-xs text-ink">{{ rosterStore.tournament?.name ?? '—' }}</span>
        </div>

        <button
          v-if="rosterStore.tournamentId !== null && !rosterStore.registered"
          class="btn-primary"
          :disabled="rosterStore.saving"
          @click="rosterStore.register()"
        >
          {{ rosterStore.saving ? 'Registering…' : 'Register to Draft' }}
        </button>
        <p v-else-if="rosterStore.locked" class="label-caps">Roster locked — read only</p>
      </div>

      <!-- Filters -->
      <div class="mt-4 flex flex-wrap items-center gap-x-6 gap-y-3">
        <div class="flex items-center gap-2">
          <span class="label-caps">Sort by</span>
          <button
            v-for="option in SORTS"
            :key="option.value"
            type="button"
            class="border px-2.5 py-1 font-mono text-[10px] font-semibold tracking-[0.1em] uppercase transition-colors"
            :class="
              heroesStore.sort === option.value
                ? 'border-cyan bg-cyan/10 text-cyan'
                : 'border-edge text-ink-dim hover:text-ink'
            "
            @click="heroesStore.sort = option.value"
          >
            {{ option.label }}
          </button>
        </div>

        <input
          v-model="heroesStore.search"
          type="search"
          placeholder="Search heroes…"
          class="w-full field-input text-xs placeholder:text-ink-dim sm:ml-auto sm:w-56"
        />
      </div>

      <p
        v-if="rosterStore.error"
        class="mt-4 border border-magenta/50 bg-magenta/10 p-3 font-mono text-xs text-magenta"
      >
        {{ rosterStore.error }}
      </p>

      <p v-if="heroesStore.loading" class="mt-6 font-mono text-sm text-ink-dim">
        Loading hero pool…
      </p>

      <div v-else class="mt-5 grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-3 xl:grid-cols-4">
        <HeroCard
          v-for="hero in heroesStore.heroes"
          :key="hero.id"
          :hero="hero"
          :selected="rosterStore.isSelected(hero.id)"
          :disabled="!rosterStore.registered || rosterStore.locked"
          @toggle="rosterStore.toggle"
        />
      </div>
    </section>

    <RosterUplinkPanel />
  </div>
</template>
