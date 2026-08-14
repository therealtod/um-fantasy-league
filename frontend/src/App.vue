<script setup lang="ts">
import { onMounted } from 'vue'
import AppShell from '@/layouts/AppShell.vue'
import { useAuthStore } from '@/stores/auth'
import { useManagerStore } from '@/stores/manager'
import { useTournamentsStore } from '@/stores/tournaments'

const authStore = useAuthStore()
const managerStore = useManagerStore()
const tournamentsStore = useTournamentsStore()

onMounted(() => {
  if (authStore.isAuthenticated) void managerStore.load()
  void tournamentsStore.load()
})
</script>

<template>
  <AppShell>
    <RouterView />
  </AppShell>
</template>
