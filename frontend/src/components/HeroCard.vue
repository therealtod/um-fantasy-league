<script setup lang="ts">
import { computed } from 'vue'
import type { Hero } from '@/api/types'
import { formatCredits } from '@/lib/format'

const props = defineProps<{
  hero: Hero
  selected: boolean
  disabled?: boolean
}>()

defineEmits<{ toggle: [id: number] }>()

const cost = computed(() => formatCredits(props.hero.cost))
const initials = computed(() =>
  props.hero.name
    .split(' ')
    .map((part) => part[0])
    .join('')
    .slice(0, 2)
    .toUpperCase(),
)
</script>

<template>
  <button
    type="button"
    class="panel group relative flex w-full flex-col text-left transition-colors"
    :class="[
      selected ? 'glow-cyan' : 'hover:border-edge-strong',
      disabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer',
    ]"
    :disabled="disabled"
    :aria-pressed="selected"
    @click="$emit('toggle', hero.id)"
  >
    <!-- Portrait: falls back to initials when no artwork is on file. -->
    <div
      class="scanline-bg relative flex h-28 items-center justify-center overflow-hidden border-b border-edge bg-surface-mid"
    >
      <img
        v-if="hero.imageUrl"
        :src="hero.imageUrl"
        :alt="hero.name"
        class="size-full object-cover"
      />
      <span v-else class="headline text-3xl text-ink-dim/40">{{ initials }}</span>

      <span
        v-if="selected"
        class="absolute right-2 bottom-2 flex size-5 items-center justify-center border border-cyan bg-surface-lowest font-mono text-[10px] text-cyan"
        aria-hidden="true"
      >
        &check;
      </span>
    </div>

    <div class="flex flex-1 flex-col p-3">
      <h4 class="headline truncate text-base uppercase" :title="hero.name">
        {{ hero.name }}
      </h4>

      <dl class="mt-3">
        <div class="flex items-baseline justify-between">
          <dt class="label-caps">Cost</dt>
          <dd class="stat-value text-sm text-ink">{{ cost }}</dd>
        </div>
      </dl>
    </div>
  </button>
</template>
