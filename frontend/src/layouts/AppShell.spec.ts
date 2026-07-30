import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import type { Tournament } from '@/api/types'

const tournaments: { value: Tournament[] } = { value: [] }

vi.mock('vue-router', () => ({
  RouterLink: { template: '<a><slot /></a>' },
  useRoute: () => ({ path: '/lobby', meta: {}, fullPath: '/lobby' }),
  useRouter: () => ({ push: vi.fn() }),
}))

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ session: null, isDevelopmentIdentityMode: true }),
}))

vi.mock('@/stores/manager', () => ({
  useManagerStore: () => ({ manager: null }),
}))

// Mirrors the real store's predicate. `myEntryStatus` is absent, not null, when
// the manager has no entry, so this must test truthiness — see tournaments.spec.ts.
vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({
    get openForRegistration() {
      return tournaments.value.filter((t) => !t.myEntryStatus && t.acceptsRegistration)
    },
  }),
}))

const baseTournament: Tournament = {
  id: 1,
  name: 'Friday Night Fight',
  format: 'BANQUEST',
  status: 'REGISTRATION_OPEN',
  startDate: '2026-08-01',
  endDate: '2026-08-02',
  capacity: 16,
  enrolled: 1,
  rosterSize: 3,
  creditGrant: 10_000,
  acceptsRegistration: true,
  // myEntryStatus absent: that is what the wire looks like with no entry.
}

async function mountShell() {
  const AppShell = (await import('./AppShell.vue')).default
  return mount(AppShell, {
    global: {},
  })
}

describe('AppShell', () => {
  it('hides the Join Tournament shortcut when the manager is already registered', async () => {
    tournaments.value = [{ ...baseTournament, myEntryStatus: 'DRAFT' }]

    const wrapper = await mountShell()

    expect(wrapper.text()).not.toContain('Join Tournament')
  })

  it('shows the Join Tournament shortcut for an unregistered open tournament', async () => {
    tournaments.value = [baseTournament]

    const wrapper = await mountShell()

    expect(wrapper.text()).toContain('Join Tournament')
  })
})
