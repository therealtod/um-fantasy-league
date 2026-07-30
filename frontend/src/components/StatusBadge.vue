<script setup lang="ts">
import { computed } from 'vue'
import type { TournamentStatus } from '@/api/types'

const props = defineProps<{ status: TournamentStatus }>()

const LABELS: Record<TournamentStatus, string> = {
  LIVE: 'Live',
  REGISTRATION_OPEN: 'Open',
  SCHEDULED: 'Starting Soon',
  COMPLETED: 'Complete',
}

const STYLES: Record<TournamentStatus, string> = {
  LIVE: 'border-cyan/50 bg-cyan/10 text-cyan',
  REGISTRATION_OPEN: 'border-lime/50 bg-lime/10 text-lime',
  SCHEDULED: 'border-magenta/50 bg-magenta/10 text-magenta',
  COMPLETED: 'border-edge bg-surface-mid text-ink-dim',
}

const label = computed(() => LABELS[props.status])
const style = computed(() => STYLES[props.status])
</script>

<template>
  <span
    class="inline-flex items-center gap-1.5 border px-2 py-1 font-mono text-[10px] font-semibold tracking-[0.1em] uppercase"
    :class="style"
  >
    <span
      v-if="status === 'LIVE'"
      class="size-1.5 animate-pulse bg-cyan"
      aria-hidden="true"
    />
    {{ label }}
  </span>
</template>
