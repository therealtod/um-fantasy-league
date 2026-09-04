<script setup lang="ts">
import { computed, onMounted, watch } from 'vue'
import { useRoute } from 'vue-router'
import BudgetMeter from '@/components/BudgetMeter.vue'
import HeroCard from '@/components/HeroCard.vue'
import RosterPanel from '@/components/RosterPanel.vue'
import RosterStepper from '@/components/RosterStepper.vue'
import StatusBadge from '@/components/StatusBadge.vue'
import ErrorBanner from '@/components/ErrorBanner.vue'
import { useHeroesStore } from '@/stores/heroes'
import { useRosterStore } from '@/stores/roster'
import { nextStep, rosterStage, UNLOCKED_ENTRY_WARNING } from '@/domain/rosterGuidance'
import { debounce } from '@/lib/debounce'
import { formatCredits } from '@/lib/format'
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

// Cost is tournament-scoped, and the roster belongs to a specific tournament too,
// so a route change re-fetches both — otherwise the grid shows one tournament's
// pool while the roster store (and any toggle it sends to the server) still
// points at the previous one.
watch(tournamentId, (id) => {
  void heroesStore.select(id)
  void rosterStore.select(id)
})

watch(
  () => heroesStore.sort,
  () => void heroesStore.load(),
)

// Search reloads on every keystroke via v-model, so it's debounced to avoid
// firing a request per character; the store itself drops any response that
// resolves out of order (see heroes.ts).
const reloadOnSearch = debounce(() => void heroesStore.load(), 300)
watch(() => heroesStore.search, reloadOnSearch)

/** The state every piece of guidance copy is derived from — see `domain/rosterGuidance.ts`. */
const guidanceState = computed(() => ({
  registered: rosterStore.registered,
  locked: rosterStore.locked,
  picked: rosterStore.selected.length,
  rosterSize: rosterStore.rosterSize,
  remaining: rosterStore.budget.remaining,
  creditGrant: rosterStore.creditGrant,
}))

const stage = computed(() => rosterStage(guidanceState.value))
const step = computed(() => nextStep(guidanceState.value))

/** Server rule breaches as display lines, so a 422 lists every problem at once. */
const violationMessages = computed(() =>
  rosterStore.violations.map((violation) => violation.message),
)

const lockedAt = computed(() => {
  const at = rosterStore.roster?.lockedAt
  if (!at) return null
  const parsed = new Date(at)
  return Number.isNaN(parsed.getTime()) ? null : parsed.toLocaleString()
})
</script>

<template>
  <div class="flex flex-col gap-6 lg:flex-row">
    <!-- Hero pool -->
    <section class="min-w-0 flex-1">
      <div class="panel sticky top-0 z-20 mb-4 p-3 lg:hidden">
        <BudgetMeter :budget="rosterStore.budget" />
      </div>

      <!-- What this page is, and where the manager stands in it -->
      <div class="panel p-5">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex min-w-0 items-center gap-3">
            <StatusBadge v-if="rosterStore.tournament" :status="rosterStore.tournament.status" />
            <h2 class="headline truncate text-lg uppercase">
              {{ rosterStore.tournament?.name ?? 'Roster' }}
            </h2>
          </div>
          <RouterLink class="font-mono text-xs text-ink-dim hover:text-ink" to="/lobby">
            &larr; All tournaments
          </RouterLink>
        </div>

        <p class="mt-2 font-mono text-xs text-ink-dim">
          Pick {{ rosterStore.rosterSize }} heroes &middot;
          {{ formatCredits(rosterStore.creditGrant) }} to spend
          <template v-if="rosterStore.tournament">
            &middot; {{ rosterStore.tournament.format }}
          </template>
        </p>

        <RosterStepper class="mt-4" :stage="stage" />

        <div class="mt-4 flex flex-wrap items-end justify-between gap-4">
          <div class="min-w-0">
            <p class="headline text-base text-cyan">{{ step.title }}</p>
            <p class="mt-1 font-mono text-xs leading-relaxed text-ink-dim">{{ step.detail }}</p>
          </div>

          <button
            v-if="rosterStore.tournamentId !== null && !rosterStore.registered"
            class="btn-primary shrink-0"
            :disabled="rosterStore.saving"
            @click="rosterStore.register()"
          >
            {{ rosterStore.saving ? 'Registering…' : 'Register to Draft' }}
          </button>
        </div>

        <!-- An unlocked entry is not a half-finished entry; it is deleted when
             the tournament goes live. Nothing else on the page says so. -->
        <p
          v-if="rosterStore.registered && !rosterStore.locked"
          class="mt-4 border border-magenta/50 bg-magenta/10 p-3 font-mono text-xs leading-relaxed text-magenta"
        >
          {{ UNLOCKED_ENTRY_WARNING }}
        </p>
      </div>

      <!-- Locked: what happens now, and where to go -->
      <div v-if="rosterStore.locked" class="panel mt-4 border-lime/50 p-5">
        <h3 class="headline text-base text-lime uppercase">Your roster is locked</h3>
        <p class="mt-2 font-mono text-xs leading-relaxed text-ink-dim">
          <template v-if="lockedAt">Locked {{ lockedAt }}. </template>
          Your heroes score points from real match results as the tournament is played — there is
          nothing left to do here.
        </p>
        <p class="mt-3 font-mono text-xs text-ink">
          {{ rosterStore.selected.map((hero) => hero.name).join(' · ') }}
        </p>
        <div class="mt-4 flex flex-wrap gap-3">
          <RouterLink class="btn-primary" to="/standings">View Standings</RouterLink>
          <RouterLink class="btn-ghost" to="/lobby">Back to Tournaments</RouterLink>
        </div>
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

      <ErrorBanner
        v-if="rosterStore.error"
        class="mt-4"
        compact
        :message="rosterStore.error"
        :violations="violationMessages"
      />
      <ErrorBanner v-if="heroesStore.error" class="mt-4" compact :message="heroesStore.error" />

      <p
        v-if="!rosterStore.registered && rosterStore.tournamentId !== null"
        class="mt-4 font-mono text-xs text-ink-dim"
      >
        Register above to start picking heroes — the pool is read-only until you do.
      </p>

      <p v-if="heroesStore.loading" class="mt-6 font-mono text-sm text-ink-dim">
        Loading hero pool…
      </p>

      <p v-else-if="heroesStore.heroes.length === 0" class="mt-6 font-mono text-sm text-ink-dim">
        <template v-if="heroesStore.search">No heroes match “{{ heroesStore.search }}”.</template>
        <template v-else>No heroes in this tournament's pool yet.</template>
      </p>

      <div
        v-else
        class="mt-5 grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-3 xl:grid-cols-4"
      >
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

    <RosterPanel />
  </div>
</template>
