<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { useManagerStore } from '@/stores/manager'
import { useTournamentsStore } from '@/stores/tournaments'

const route = useRoute()
const router = useRouter()
const managerStore = useManagerStore()
const authStore = useAuthStore()
const tournamentsStore = useTournamentsStore()

async function signOut() {
  await authStore.signOut()
  void router.push({ name: 'login' })
}

function signIn() {
  void router.push({ name: 'login', query: { redirect: route.fullPath } })
}

const manager = computed(() => managerStore.manager)
const title = computed(() => (route.meta.title as string | undefined) ?? 'Mission Control')

const nav = computed(() => {
  const baseNav = [
    { to: '/lobby', label: 'Lobby' },
    { to: '/standings', label: 'Standings' },
  ]
  if (manager.value?.isAdmin) {
    baseNav.push({ to: '/admin', label: 'Admin' })
  }
  return baseNav
})

/* Below `md` the rail is an off-canvas drawer; at `md` and up it is a static
 * column and this flag is inert. */
const navOpen = ref(false)

watch(() => route.fullPath, () => (navOpen.value = false))

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') navOpen.value = false
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onUnmounted(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <div class="flex min-h-screen bg-surface text-ink">
    <!-- Scrim: only ever rendered while the drawer is open, i.e. below `md`. -->
    <div
      v-if="navOpen"
      class="fixed inset-0 z-30 bg-black/60 md:hidden"
      aria-hidden="true"
      @click="navOpen = false"
    />

    <!-- Left rail — off-canvas drawer below `md`, static column above it.
         `invisible` is what takes the closed drawer out of the tab order;
         translating alone would leave the links focusable off-screen. -->
    <aside
      id="app-nav"
      class="fixed inset-y-0 left-0 z-40 flex w-56 flex-col border-r border-edge bg-surface-lowest transition-transform duration-200 md:visible md:static md:shrink-0 md:translate-x-0"
      :class="navOpen ? 'visible translate-x-0' : 'invisible -translate-x-full'"
    >
      <div class="border-b border-edge px-5 py-6">
        <h1 class="headline text-lg leading-tight uppercase">UM Fantasy<br />League</h1>
        <p class="label-caps mt-2">Work in progress</p>
      </div>

      <nav class="flex-1 py-4">
        <template v-for="item in nav" :key="item.to">
          <RouterLink v-slot="{ isActive, href, navigate }" :to="item.to" custom>
            <a
              :href="href"
              class="relative flex items-center gap-3 px-5 py-3 transition-colors"
              :class="
                isActive
                  ? 'bg-surface-low text-cyan'
                  : 'text-ink-muted hover:bg-surface-low hover:text-ink'
              "
              @click="navigate"
            >
              <span v-if="isActive" class="absolute inset-y-0 left-0 w-[2px] bg-cyan" aria-hidden="true" />
              <span class="font-mono text-xs font-semibold tracking-[0.1em] uppercase">
                {{ item.label }}
              </span>
            </a>
          </RouterLink>
        </template>
      </nav>

      <div v-if="tournamentsStore.openForRegistration.length > 0" class="border-t border-edge p-4">
        <RouterLink to="/lobby" class="btn-primary block w-full text-center">
          Join Tournament
        </RouterLink>
      </div>
    </aside>

    <!-- Main column -->
    <div class="flex min-w-0 flex-1 flex-col">
      <header
        class="flex items-center justify-between gap-3 border-b border-edge bg-surface-lowest px-4 py-3 md:gap-6 md:px-8 md:py-4"
      >
        <button
          type="button"
          class="btn-ghost shrink-0 px-3 md:hidden"
          aria-label="Open navigation"
          aria-controls="app-nav"
          :aria-expanded="navOpen"
          @click="navOpen = !navOpen"
        >
          ☰
        </button>

        <h2 class="headline min-w-0 flex-1 truncate text-lg uppercase md:text-2xl">{{ title }}</h2>

        <div class="flex shrink-0 items-center gap-3">
          <div
            class="flex h-10 shrink-0 items-center justify-center whitespace-nowrap border border-edge bg-surface-mid px-3 font-mono text-sm font-bold text-cyan"
          >
            {{ manager?.handle?.toUpperCase() ?? '··' }}
          </div>
          <button
            v-if="authStore.session && !authStore.isDevelopmentIdentityMode"
            type="button"
            class="btn-ghost"
            @click="signOut"
          >
            Sign out
          </button>
          <button
            v-else-if="!authStore.isDevelopmentIdentityMode"
            type="button"
            class="btn-primary"
            @click="signIn"
          >
            Sign in
          </button>
        </div>
      </header>

      <main class="min-w-0 flex-1 overflow-y-auto p-4 md:p-8">
        <slot />
      </main>
    </div>
  </div>
</template>
