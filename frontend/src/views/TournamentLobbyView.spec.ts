import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Tournament } from '@/api/types'

const push = vi.fn()
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))

vi.mock('@/api/client', () => ({
  api: { tournaments: vi.fn().mockResolvedValue([]), register: vi.fn(), entry: vi.fn() },
  ApiError: class extends Error {
    status = 0
    violations: unknown[] = []
  },
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
}))

import TournamentLobbyView from './TournamentLobbyView.vue'
import { useTournamentsStore } from '@/stores/tournaments'

/**
 * The lobby card's *destination*, which is what this file exists to pin.
 *
 * `swapInviteOpen` is already tested as data in `domain/tournamentStatus.spec.ts`;
 * the bug this guards is the wiring — a live tournament sending a locked entry to
 * `/standings` when its swap window is open, which no test of the predicate alone
 * could catch. So this mounts, like `MatchResultWizard.spec.ts`, only to assert
 * the join between a rule and the button that acts on it.
 */
function live(overrides: Partial<Tournament> = {}): Tournament {
  return {
    id: 7,
    name: 'Winter of Champions',
    format: 'BANQUEST',
    status: 'LIVE',
    startDate: '2026-01-10',
    capacity: 16,
    enrolled: 12,
    rosterSize: 3,
    creditGrant: 10_000,
    acceptsRegistration: false,
    currentRound: 3,
    swapsPerRound: 1,
    swapWindowOpen: true,
    myEntryStatus: 'LOCKED',
    ...overrides,
  }
}

async function renderCard(tournament: Tournament) {
  const wrapper = mount(TournamentLobbyView, {
    global: { stubs: { StatusBadge: true, ErrorBanner: true } },
  })
  useTournamentsStore().tournaments = [tournament]
  await wrapper.vm.$nextTick()
  return wrapper
}

describe('TournamentLobbyView swap routing', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    push.mockClear()
  })

  it('sends a locked entry to the roster while the swap window is open', async () => {
    const wrapper = await renderCard(live())
    const button = wrapper.get('button')

    expect(button.text()).toBe('Swap Heroes')
    await button.trigger('click')
    expect(push).toHaveBeenCalledWith('/tournaments/7/roster')
  })

  it('falls back to spectating once the window is shut', async () => {
    const wrapper = await renderCard(live({ swapWindowOpen: false }))
    const button = wrapper.get('button')

    expect(button.text()).toBe('Enter Spectator')
    await button.trigger('click')
    expect(push).toHaveBeenCalledWith('/standings')
  })

  it('flags the open window on the card', async () => {
    expect((await renderCard(live())).text()).toContain('Swap Window')
    expect((await renderCard(live({ swapWindowOpen: false }))).text()).not.toContain('Swap Window')
  })
})
