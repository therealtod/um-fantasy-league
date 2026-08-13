<script setup lang="ts">
defineProps<{
  title: string
  busy?: boolean
  busyLabel: string
  confirmLabel: string
}>()

defineEmits<{
  cancel: []
  confirm: []
}>()
</script>

<template>
  <div class="panel flex flex-col gap-5 border-magenta p-6">
    <h3 class="headline text-lg text-magenta">{{ title }}</h3>
    <p class="font-mono text-sm leading-relaxed text-ink-dim">
      <slot />
    </p>
    <div class="flex justify-end gap-3 pt-2">
      <button class="btn-ghost" :disabled="busy" @click="$emit('cancel')">Cancel</button>
      <button
        class="border border-magenta px-6 py-3 font-mono text-sm text-magenta transition-opacity hover:opacity-85 disabled:cursor-not-allowed disabled:opacity-50"
        :disabled="busy"
        @click="$emit('confirm')"
      >
        {{ busy ? busyLabel : confirmLabel }}
      </button>
    </div>
  </div>
</template>
