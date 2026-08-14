<script setup lang="ts">
import { computed } from 'vue'
import BudgetMeter from './BudgetMeter.vue'
import { useRosterStore } from '@/stores/roster'
import { formatCredits } from '@/lib/format'

const roster = useRosterStore()

const emptySlots = computed(() => Math.max(0, roster.rosterSize - roster.selected.length))

const averageCost = computed(() =>
  roster.selected.length === 0 ? 0 : Math.round(roster.budget.spent / roster.selected.length),
)
</script>

<template>
  <aside class="panel flex w-full flex-col lg:w-80 lg:shrink-0 lg:self-start">
    <header class="border-b border-edge px-5 py-4">
      <h3 class="headline text-base uppercase">Roster Uplink</h3>
      <p class="label-caps mt-1">
        {{ roster.locked ? 'Deployment confirmed' : 'Confirm deployment' }}
      </p>
    </header>

    <div class="border-b border-edge px-5 py-4">
      <BudgetMeter :budget="roster.budget" />
    </div>

    <!-- Slots -->
    <ul class="space-y-2 px-5 py-4">
      <li
        v-for="(hero, index) in roster.selected"
        :key="hero.id"
        class="flex items-center gap-3 border border-edge bg-surface-mid p-2.5"
      >
        <span class="label-caps w-4 shrink-0">{{ index + 1 }}</span>
        <p class="min-w-0 flex-1 truncate font-mono text-xs font-bold text-ink uppercase">
          {{ hero.name }}
        </p>
        <span class="stat-value shrink-0 text-xs text-cyan">{{ formatCredits(hero.cost) }}</span>
        <button
          v-if="!roster.locked"
          type="button"
          class="shrink-0 px-1 font-mono text-xs text-ink-dim transition-colors hover:text-magenta"
          :aria-label="`Remove ${hero.name}`"
          @click="roster.toggle(hero.id)"
        >
          &times;
        </button>
      </li>

      <li
        v-for="slot in emptySlots"
        :key="`empty-${slot}`"
        class="flex items-center justify-center border border-dashed border-edge bg-surface-lowest/50 p-4"
      >
        <span class="label-caps">Awaiting input…</span>
      </li>
    </ul>

    <!-- Aggregates -->
    <div class="border-t border-edge px-5 py-4">
      <div class="border border-edge bg-surface-mid p-2.5">
        <p class="label-caps">Avg Cost</p>
        <p class="stat-value mt-1 text-sm text-ink">{{ formatCredits(averageCost) }}</p>
      </div>
    </div>

    <!-- Rule breaches reported by the server -->
    <ul v-if="roster.violations.length" class="border-t border-edge px-5 py-3">
      <li
        v-for="violation in roster.violations"
        :key="violation.rule"
        class="font-mono text-[11px] text-magenta"
      >
        {{ violation.message }}
      </li>
    </ul>

    <div class="border-t border-edge p-4">
      <button
        v-if="!roster.locked"
        class="btn-primary w-full"
        :disabled="!roster.lockable || roster.saving"
        @click="roster.lock()"
      >
        <template v-if="roster.saving">Working…</template>
        <template v-else>
          Lock Roster ({{ roster.selected.length }}/{{ roster.rosterSize }})
        </template>
      </button>

      <p v-else class="border border-lime/50 bg-lime/10 py-3 text-center font-mono text-xs tracking-[0.1em] text-lime uppercase">
        Roster Locked
      </p>
    </div>
  </aside>
</template>
