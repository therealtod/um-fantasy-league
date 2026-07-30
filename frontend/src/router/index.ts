import { createRouter, createWebHistory } from 'vue-router'
import TournamentLobbyView from '@/views/TournamentLobbyView.vue'
import LoginView from '@/views/LoginView.vue'
import { useAuthStore } from '@/stores/auth'
import { useTournamentsStore } from '@/stores/tournaments'
import { useManagerStore } from '@/stores/manager'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', redirect: '/lobby' },
    {
      path: '/login',
      name: 'login',
      component: LoginView,
      meta: { public: true },
    },
    {
      path: '/lobby',
      name: 'lobby',
      component: TournamentLobbyView,
      meta: { title: 'Tournament Lobby', public: true },
    },
    {
      path: '/tournaments/:tournamentId/roster',
      name: 'roster',
      component: () => import('@/views/RosterBuilderView.vue'),
      meta: { title: 'Roster Builder' },
      beforeEnter: async (to) => {
        const id = Number(to.params.tournamentId)
        if (!Number.isInteger(id)) return { name: 'lobby' }

        const tournamentsStore = useTournamentsStore()
        if (tournamentsStore.tournaments.length === 0) {
          await tournamentsStore.load()
        }

        const tournament = tournamentsStore.byId(id)
        // `myEntryStatus` is absent rather than null when there is no entry.
        const accessible =
          tournament !== null && (Boolean(tournament.myEntryStatus) || tournament.acceptsRegistration)
        if (!accessible) return { name: 'lobby' }
      },
    },
    {
      path: '/standings',
      name: 'standings',
      component: () => import('@/views/StandingsView.vue'),
      meta: { title: 'Live Standings', public: true },
    },
    {
      path: '/admin',
      name: 'admin',
      component: () => import('@/views/AdminDashboardView.vue'),
      meta: { title: 'Admin Command Center', requiresAdmin: true },
      beforeEnter: async () => {
        const managerStore = useManagerStore()
        if (!managerStore.manager) {
          await managerStore.load()
        }
        if (!managerStore.manager?.isAdmin) {
          return { name: 'lobby' }
        }
      },
    },
  ],
})

router.beforeEach((to) => {
  const authStore = useAuthStore()
  if (authStore.error && !authStore.isAuthenticated && to.name !== 'login') {
    return { name: 'login' }
  }
  if (!to.meta.public && !authStore.isAuthenticated) {
    return { name: 'login', query: { redirect: to.fullPath } }
  }
  if (to.name === 'login' && authStore.isAuthenticated) {
    return { name: 'lobby' }
  }
})

export default router
