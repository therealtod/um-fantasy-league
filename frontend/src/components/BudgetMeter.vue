<script setup lang="ts">
import { computed } from 'vue'
import type { BudgetStatus } from '@/api/types'
import { formatCredits } from '@/lib/format'

const props = defineProps<{ budget: BudgetStatus }>()

const percent = computed(() => Math.min(100, Math.max(0, props.budget.utilisation * 100)))
const over = computed(() => props.budget.remaining < 0)
</script>

<template>
  <div>
    <div class="flex items-baseline justify-between">
      <span class="label-caps">Budget</span>
      <span class="stat-value text-sm">
        <span :class="over ? 'text-magenta' : 'text-cyan'">{{ formatCredits(budget.spent) }}</span>
        <span class="text-ink-dim"> / {{ formatCredits(budget.creditGrant) }}</span>
      </span>
    </div>

    <div class="mt-2 h-1.5 w-full bg-surface-high">
      <div
        class="h-full transition-[width] duration-200"
        :class="over ? 'bg-magenta' : 'bg-cyan'"
        :style="{ width: `${percent}%` }"
      />
    </div>

    <p class="mt-2 font-mono text-[11px]" :class="over ? 'text-magenta' : 'text-ink-dim'">
      {{
        over
          ? `${formatCredits(-budget.remaining)} over budget`
          : `${formatCredits(budget.remaining)} remaining`
      }}
    </p>
  </div>
</template>
