<script setup lang="ts">
import { computed } from 'vue'
import type { Tournament } from '@/api/types'

const props = withDefaults(
  defineProps<{
    modelValue: number | null
    tournaments: Tournament[]
    id?: string
    label?: string
  }>(),
  {
    id: 'tournament-select',
    label: 'Select Tournament *',
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: number | null]
}>()

const selected = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value),
})
</script>

<template>
  <div class="flex flex-col gap-2">
    <label :for="id" class="label-caps">{{ label }}</label>
    <select :id="id" v-model.number="selected" class="cursor-pointer field-input">
      <option :value="null">-- Choose a tournament --</option>
      <option v-for="tournament in tournaments" :key="tournament.id" :value="tournament.id">
        {{ tournament.name }}
      </option>
    </select>
  </div>
</template>
