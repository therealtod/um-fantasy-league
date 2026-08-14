<script setup lang="ts">
defineProps<{
  message?: string | null
  /** Structured rule violations, one line each — see `violationMessages` in `api/client.ts`. */
  violations?: string[]
  compact?: boolean
}>()
</script>

<template>
  <div
    v-if="message"
    class="border border-magenta/50 bg-magenta/10 font-mono text-magenta"
    :class="compact ? 'p-3 text-xs' : 'p-4 text-sm'"
  >
    <ul v-if="violations && violations.length > 1" class="list-disc space-y-1 pl-4">
      <li v-for="violation in violations" :key="violation">{{ violation }}</li>
    </ul>
    <p v-else>{{ violations?.[0] ?? message }}</p>
  </div>
</template>
