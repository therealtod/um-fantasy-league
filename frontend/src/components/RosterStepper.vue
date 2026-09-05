<script setup lang="ts">
import { computed } from 'vue'
import type { RosterStage } from '@/domain/rosterGuidance'

/**
 * The steps of an entry, so a manager can see where they are and what is still
 * ahead of them. `stage` comes from `rosterStage()` — this component decides
 * nothing, it only paints.
 *
 * `SWAP` is the one step that is not always on the board. It follows `DONE`
 * rather than replacing it, because a locked roster genuinely is finished until
 * an admin opens a window, and appears only while the stage says so — which
 * keeps the decision in `rosterStage` where the rest of them live.
 */
const props = defineProps<{ stage: RosterStage }>()

type StepState = 'done' | 'current' | 'todo'

const ENTRY_STEPS: { stage: RosterStage; label: string }[] = [
  { stage: 'REGISTER', label: 'Register' },
  { stage: 'PICK', label: 'Pick heroes' },
  { stage: 'LOCK', label: 'Lock roster' },
  { stage: 'DONE', label: 'Watch standings' },
]

const SWAP_STEP: { stage: RosterStage; label: string } = { stage: 'SWAP', label: 'Swap heroes' }

const steps = computed(() =>
  props.stage === 'SWAP' ? [...ENTRY_STEPS, SWAP_STEP] : ENTRY_STEPS,
)

const STYLES: Record<StepState, string> = {
  done: 'border-lime/50 bg-lime/10 text-lime',
  current: 'border-cyan bg-cyan/10 text-cyan',
  todo: 'border-edge text-ink-dim',
}

const currentIndex = computed(() => steps.value.findIndex((step) => step.stage === props.stage))

function stateOf(index: number): StepState {
  if (index < currentIndex.value) return 'done'
  return index === currentIndex.value ? 'current' : 'todo'
}
</script>

<template>
  <ol class="grid grid-cols-2 gap-2" :class="steps.length === 5 ? 'sm:grid-cols-5' : 'sm:grid-cols-4'">
    <li
      v-for="(step, index) in steps"
      :key="step.stage"
      class="flex min-w-0 items-center gap-2 border px-3 py-2"
      :class="STYLES[stateOf(index)]"
      :aria-current="stateOf(index) === 'current' ? 'step' : undefined"
    >
      <span class="shrink-0 font-mono text-[10px] font-bold" aria-hidden="true">
        {{ stateOf(index) === 'done' ? '✓' : index + 1 }}
      </span>
      <span class="truncate font-mono text-[10px] font-semibold tracking-[0.1em] uppercase">
        {{ step.label }}
      </span>
    </li>
  </ol>
</template>
