import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Hero, MapAdminDto, MatchResultDto } from '@/api/types'

const listMapPool = vi.fn()
const heroes = vi.fn()
const recordMatch = vi.fn()
const correctMatch = vi.fn()
const getMatch = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    heroes: (...args: unknown[]) => heroes(...args),
    admin: {
      listMapPool: (...args: unknown[]) => listMapPool(...args),
      recordMatch: (...args: unknown[]) => recordMatch(...args),
      correctMatch: (...args: unknown[]) => correctMatch(...args),
      getMatch: (...args: unknown[]) => getMatch(...args),
    },
  },
}))

vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({ tournaments: [] }),
}))

const mapPool: MapAdminDto[] = [{ id: 5, name: 'Center Square' }]
const heroPool: Hero[] = [
  { id: 10, name: 'Sherlock Holmes', imageUrl: null, cost: 500 },
  { id: 11, name: 'Dracula', imageUrl: null, cost: 400 },
]

async function mountWizard(props: { tournamentId?: number; matchId?: number; mode: 'create' | 'edit' }) {
  const MatchResultWizard = (await import('./MatchResultWizard.vue')).default
  const wrapper = mount(MatchResultWizard, { props })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  listMapPool.mockResolvedValue(mapPool)
  heroes.mockResolvedValue(heroPool)
})

describe('MatchResultWizard', () => {
  it('loads the tournament\'s map and hero pools instead of asking for raw ids', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    expect(listMapPool).toHaveBeenCalledWith(1)
    expect(heroes).toHaveBeenCalledWith(1)
    // No more raw-id number inputs for map or hero — every id comes from a <select>.
    expect(wrapper.find('#match-map').element.tagName).toBe('SELECT')
    expect(wrapper.find('#hero-0').element.tagName).toBe('SELECT')
    expect(wrapper.text()).toContain('Center Square')
    expect(wrapper.text()).toContain('Sherlock Holmes')
  })

  it('maps a filled-in form to RecordMatchRequest on create', async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#match-map').setValue('5')
    await wrapper.find('#hero-0').setValue('10')
    await wrapper.find('#hero-1').setValue('11')
    await wrapper.find('#health-0').setValue('12')
    await wrapper.find('#health-1').setValue('0')
    await wrapper.findAll('input[type=checkbox]')[0]!.setValue(true)

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).toHaveBeenCalledWith(
      1,
      expect.objectContaining({
        mapId: 5,
        participants: [
          expect.objectContaining({ heroId: 10, healthRemaining: 12, isWinner: true }),
          expect.objectContaining({ heroId: 11, healthRemaining: 0, isWinner: false }),
        ],
      }),
    )
  })

  it('refuses to save while a participant has no hero selected', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#match-map').setValue('5')
    // Heroes are left at the placeholder (id 0).
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('All participants must have a hero selected')
  })

  it('round-trips an existing match through getMatch in edit mode', async () => {
    const existing: MatchResultDto = {
      matchId: 42,
      tournamentId: 1,
      round: 2,
      mapId: 5,
      mapName: 'Center Square',
      playedAt: '2026-08-01T12:00:00Z',
      participants: [
        {
          participantId: 1,
          playerLabel: 'Alice',
          heroId: 10,
          heroName: 'Sherlock Holmes',
          healthRemaining: 8,
          isWinner: true,
        },
        {
          participantId: 2,
          heroId: 11,
          heroName: 'Dracula',
          healthRemaining: 0,
          isWinner: false,
        },
      ],
      bans: [{ heroId: 11, heroName: 'Dracula' }],
    }
    getMatch.mockResolvedValue(existing)
    correctMatch.mockResolvedValue(existing)

    const wrapper = await mountWizard({ tournamentId: 1, matchId: 42, mode: 'edit' })

    expect(getMatch).toHaveBeenCalledWith(1, 42)
    expect((wrapper.find('#match-map').element as HTMLSelectElement).value).toBe('5')
    expect((wrapper.find('#hero-0').element as HTMLSelectElement).value).toBe('10')
    expect((wrapper.find('#health-0').element as HTMLInputElement).value).toBe('8')
    expect((wrapper.find('#ban-hero-0').element as HTMLSelectElement).value).toBe('11')

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(correctMatch).toHaveBeenCalledWith(
      1,
      42,
      expect.objectContaining({
        mapId: 5,
        participants: [
          expect.objectContaining({ playerLabel: 'Alice', heroId: 10, healthRemaining: 8, isWinner: true }),
          expect.objectContaining({ heroId: 11, healthRemaining: 0, isWinner: false }),
        ],
        bans: [{ heroId: 11 }],
      }),
    )
  })
})
