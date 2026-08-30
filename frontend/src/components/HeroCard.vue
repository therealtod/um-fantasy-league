<script setup lang="ts">
import { computed, ref, watch } from 'vue'
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

/**
 * `image_url` is free text an admin typed, so a URL that 404s is expected rather
 * than exceptional — a failed load falls back to the same initials a hero with no
 * artwork gets. The grid reuses card instances across tournaments, so the flag has
 * to reset whenever the URL itself changes or a stale failure outlives its hero.
 */
const imageFailed = ref(false)
watch(
  () => props.hero.imageUrl,
  () => {
    imageFailed.value = false
  },
)

const showImage = computed(() => Boolean(props.hero.imageUrl) && !imageFailed.value)
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
    <!--
      Portrait: a fixed 3:4 box, so every card in the grid is the same height whatever
      the artwork measures. The image is `object-contain` rather than `object-cover` —
      hero art is not authored to this ratio and cropping it loses the top of a portrait —
      and the scanline backdrop is what fills the letterbox gaps. Falls back to initials
      when no artwork is on file or the URL fails to load.
    -->
    <div
      class="scanline-bg relative flex aspect-[3/4] items-center justify-center overflow-hidden border-b border-edge bg-surface-mid"
    >
      <img
        v-if="showImage"
        :src="hero.imageUrl!"
        alt=""
        loading="lazy"
        decoding="async"
        class="size-full object-contain"
        @error="imageFailed = true"
      />
      <span v-else class="headline text-4xl text-ink-dim/40">{{ initials }}</span>

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
