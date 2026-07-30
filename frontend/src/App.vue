<script setup lang="ts">
import { onMounted } from 'vue'
import AppShell from '@/layouts/AppShell.vue'
import { useManagerStore } from '@/stores/manager'
import { useTournamentsStore } from '@/stores/tournaments'

const managerStore = useManagerStore()
const tournamentsStore = useTournamentsStore()

onMounted(() => {
  void managerStore.load()
  void tournamentsStore.load()
})
</script>

<template>
  <AppShell>
    <RouterView />
  </AppShell>
</template>
