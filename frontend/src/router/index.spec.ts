import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Session } from '@supabase/supabase-js'

const currentSession: { value: Session | null } = { value: null }

// Router/store setup imports the real Supabase client transitively (via the
// api stores), which throws at import time without VITE_SUPABASE_* env vars.
// Mocking both keeps this a pure routing test with no network dependency.
vi.mock('@/lib/supabaseClient', () => ({
  supabase: {
    auth: {
      getSession: vi.fn().mockResolvedValue({ data: { session: null } }),
      onAuthStateChange: vi.fn(),
      signInWithOAuth: vi.fn(),
      signOut: vi.fn(),
    },
  },
}))

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    get isAuthenticated() {
      return currentSession.value !== null
    },
  }),
}))

// The roster route's beforeEnter looks up the tournament directly, so it needs
// its own mock rather than hitting the real store/API.
vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({
    tournaments: [{ id: 1, myEntryStatus: 'DRAFT', acceptsRegistration: false }],
    load: vi.fn(),
    byId: (id: number) => (id === 1 ? { id: 1, myEntryStatus: 'DRAFT', acceptsRegistration: false } : null),
  }),
}))

const { default: router } = await import('./index')

describe('router auth guard', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    currentSession.value = null
  })

  it('redirects an unauthenticated visit to a protected route to /login', async () => {
    await router.push('/tournaments/1/roster')
    expect(router.currentRoute.value.name).toBe('login')
  })

  it('allows an authenticated visit to a protected route', async () => {
    currentSession.value = { access_token: 'token' } as Session
    await router.push('/tournaments/1/roster')
    expect(router.currentRoute.value.name).toBe('roster')
  })

  it('redirects an authenticated visit to /login onward to /lobby', async () => {
    currentSession.value = { access_token: 'token' } as Session
    await router.push('/login')
    expect(router.currentRoute.value.name).toBe('lobby')
  })

  it('sends an authenticated visit to /login onward to a same-origin redirect target', async () => {
    currentSession.value = { access_token: 'token' } as Session
    await router.push({ path: '/login', query: { redirect: '/standings' } })
    expect(router.currentRoute.value.name).toBe('standings')
  })

  it('falls back to /lobby when the redirect target is not a safe same-origin path', async () => {
    currentSession.value = { access_token: 'token' } as Session
    await router.push({ path: '/login', query: { redirect: '//evil.example.com' } })
    expect(router.currentRoute.value.name).toBe('lobby')
  })

  it('allows an unauthenticated visit to /login', async () => {
    await router.push('/login')
    expect(router.currentRoute.value.name).toBe('login')
  })

  it('allows an unauthenticated visit to the public lobby', async () => {
    await router.push('/lobby')
    expect(router.currentRoute.value.name).toBe('lobby')
  })
})
