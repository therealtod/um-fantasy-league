import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Session } from '@supabase/supabase-js'

const managerReset = vi.fn()
const rosterClearSession = vi.fn()

vi.mock('@/lib/developmentIdentity', () => ({
  isDevelopmentIdentityMode: false,
}))

let authStateCallback: (event: string, session: Session | null) => void = () => {}

vi.mock('@/lib/supabaseClient', () => ({
  supabase: {
    auth: {
      getSession: vi.fn().mockResolvedValue({ data: { session: null } }),
      onAuthStateChange: vi.fn((cb: (event: string, session: Session | null) => void) => {
        authStateCallback = cb
        return { data: { subscription: { unsubscribe: vi.fn() } } }
      }),
      signOut: vi.fn().mockResolvedValue({ error: null }),
    },
  },
}))

vi.mock('@/stores/manager', () => ({
  useManagerStore: () => ({ reset: managerReset }),
}))

vi.mock('@/stores/roster', () => ({
  useRosterStore: () => ({ clearSession: rosterClearSession }),
}))

const fakeSession = { access_token: 'token' } as Session

describe('auth store', () => {
  beforeEach(async () => {
    setActivePinia(createPinia())
    managerReset.mockClear()
    rosterClearSession.mockClear()

    const { useAuthStore } = await import('./auth')
    const auth = useAuthStore()
    await auth.init()
  })

  it('does not reset the manager/roster stores on the initial sign-in', async () => {
    const { useAuthStore } = await import('./auth')
    const auth = useAuthStore()

    authStateCallback('SIGNED_IN', fakeSession)
    expect(auth.session).toEqual(fakeSession)
    expect(managerReset).not.toHaveBeenCalled()
    expect(rosterClearSession).not.toHaveBeenCalled()
  })

  it('resets the manager and roster stores when a session drops', async () => {
    const { useAuthStore } = await import('./auth')
    const auth = useAuthStore()

    authStateCallback('SIGNED_IN', fakeSession)
    authStateCallback('SIGNED_OUT', null)
    expect(managerReset).toHaveBeenCalledTimes(1)
    expect(rosterClearSession).toHaveBeenCalledTimes(1)
    expect(auth.session).toBeNull()
  })

  it('resets on a token expiring via onAuthStateChange, the same path as an explicit sign-out', async () => {
    const { useAuthStore } = await import('./auth')
    useAuthStore()

    authStateCallback('SIGNED_IN', fakeSession)
    authStateCallback('TOKEN_REFRESHED', null)
    expect(managerReset).toHaveBeenCalledTimes(1)
    expect(rosterClearSession).toHaveBeenCalledTimes(1)
  })
})
