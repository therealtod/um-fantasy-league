<script setup lang="ts">
import { useRoute } from 'vue-router'
import { useAuthStore } from '@/stores/auth'

const authStore = useAuthStore()
const route = useRoute()

function signIn() {
  const redirect = route.query.redirect
  void authStore.signInWithDiscord(typeof redirect === 'string' ? redirect : undefined)
}
</script>

<template>
  <div class="flex min-h-full items-center justify-center">
    <div class="panel w-full max-w-sm p-8 text-center">
      <h1 class="headline text-2xl uppercase">Mission<br />Control</h1>
      <p class="label-caps mt-2">Tactical Analytics</p>

      <p class="mt-6 text-sm text-ink-muted">
        Authenticate to draft rosters and enter tournaments.
      </p>

      <button type="button" class="btn-primary mt-8 w-full" @click="signIn">
        Sign in with Discord
      </button>

      <p v-if="authStore.error" class="mt-4 text-sm text-magenta">
        {{ authStore.error }}
      </p>
    </div>
  </div>
</template>
