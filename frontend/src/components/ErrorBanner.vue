<script setup lang="ts">
defineProps<{
  message?: string | null
  compact?: boolean
}>()
</script>

<template>
  <p
    v-if="message"
    class="border border-magenta/50 bg-magenta/10 font-mono text-magenta"
    :class="compact ? 'p-3 text-xs' : 'p-4 text-sm'"
  >
    {{ message }}
  </p>
</template>
