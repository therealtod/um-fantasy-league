import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Hero, MapAdminDto, MatchResultDto } from '@/api/types'

const listMapPool = vi.fn()
const listHeroPool = vi.fn()
const recordMatch = vi.fn()
const correctMatch = vi.fn()
const getMatch = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      listMapPool: (...args: unknown[]) => listMapPool(...args),
      listHeroPool: (...args: unknown[]) => listHeroPool(...args),
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
  listHeroPool.mockResolvedValue(heroPool)
})

describe('MatchResultWizard', () => {
  it('loads the tournament\'s map and hero pools instead of asking for raw ids', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    // Both pools come from the admin surface, never the player-facing /api/tournaments/{id}/heroes.
    expect(listMapPool).toHaveBeenCalledWith(1)
    expect(listHeroPool).toHaveBeenCalledWith(1)
    // No more raw-id number inputs for map or hero — every id comes from a <select>.
    expect(wrapper.find('#game-0-map').element.tagName).toBe('SELECT')
    expect(wrapper.find('#game-0-hero-0').element.tagName).toBe('SELECT')
    expect(wrapper.text()).toContain('Center Square')
    expect(wrapper.text()).toContain('Sherlock Holmes')
  })

  it('maps a filled-in form to RecordMatchRequest on create', async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    await wrapper.find('#game-0-health-0').setValue('12')
    await wrapper.find('#game-0-health-1').setValue('0')
    await wrapper.findAll('input[type=radio]')[0]!.setValue()

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).toHaveBeenCalledWith(
      1,
      expect.objectContaining({
        games: [
          expect.objectContaining({
            gameNumber: 1,
            mapId: 5,
            participants: [
              expect.objectContaining({ heroId: 10, healthRemaining: 12, isWinner: true }),
              expect.objectContaining({ heroId: 11, healthRemaining: 0, isWinner: false }),
            ],
          }),
        ],
      }),
    )
  })

  it('refuses to save a game with no winner picked, rather than posting a draw', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    // Winner left untouched — the server would reject this as NOT_EXACTLY_ONE_WINNER.

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('cannot end in a draw')
  })

  it('refuses to save a game with a positive-health loser', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    await wrapper.find('#game-0-health-1').setValue('1')
    await wrapper.findAll('input[type=radio]')[0]!.setValue()

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('losing hero must have 0 or less health')
  })

  it('moves the winner to the other side instead of ever having two', async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')

    const winnerRadios = () => wrapper.findAll('input[type=radio]')
    await winnerRadios()[0]!.setValue()
    await winnerRadios()[1]!.setValue()

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).toHaveBeenCalledWith(
      1,
      expect.objectContaining({
        games: [
          expect.objectContaining({
            participants: [
              expect.objectContaining({ heroId: 10, isWinner: false }),
              expect.objectContaining({ heroId: 11, isWinner: true }),
            ],
          }),
        ],
      }),
    )
  })

  it('refuses to save while a game participant has no hero selected', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#game-0-map').setValue('5')
    // Heroes are left at the placeholder (id 0).
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Every game needs a hero selected for both sides')
  })

  it('the template never offers to remove the only game, so at least one always survives', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    expect(wrapper.findAll('button').some((b) => b.text() === 'Remove Game')).toBe(false)
  })

  it('adds and removes games, renumbering the rest', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.findAll('button')
      .find((b) => b.text() === '+ Add Game')!
      .trigger('click')
    await wrapper.findAll('button')
      .find((b) => b.text() === '+ Add Game')!
      .trigger('click')

    expect(wrapper.findAll('h5').map((h) => h.text())).toEqual(['Game 1', 'Game 2', 'Game 3'])

    const removeButtons = wrapper.findAll('button').filter((b) => b.text() === 'Remove Game')
    await removeButtons[0]!.trigger('click')

    expect(wrapper.findAll('h5').map((h) => h.text())).toEqual(['Game 1', 'Game 2'])
  })

  it('round-trips an existing best-of-two match through getMatch in edit mode', async () => {
    const existing: MatchResultDto = {
      matchId: 42,
      tournamentId: 1,
      round: 2,
      playedAt: '2026-08-01T12:00:00Z',
      externalLink: 'https://example.com/bracket/9',
      participants: [
        { side: 0, playerLabel: 'Alice' },
        { side: 1 },
      ],
      games: [
        {
          gameId: 100,
          gameNumber: 1,
          mapId: 5,
          mapName: 'Center Square',
          participants: [
            { side: 0, heroId: 10, heroName: 'Sherlock Holmes', healthRemaining: 8, isWinner: true },
            { side: 1, heroId: 11, heroName: 'Dracula', healthRemaining: 0, isWinner: false },
          ],
        },
      ],
      bans: [{ heroId: 11, heroName: 'Dracula', banType: 'OPPONENT_BAN' }],
    }
    getMatch.mockResolvedValue(existing)
    correctMatch.mockResolvedValue(existing)

    const wrapper = await mountWizard({ tournamentId: 1, matchId: 42, mode: 'edit' })

    expect(getMatch).toHaveBeenCalledWith(1, 42)
    expect((wrapper.find('#match-external-link').element as HTMLInputElement).value).toBe(
      'https://example.com/bracket/9',
    )
    expect((wrapper.find('#game-0-map').element as HTMLSelectElement).value).toBe('5')
    expect((wrapper.find('#game-0-hero-0').element as HTMLSelectElement).value).toBe('10')
    expect((wrapper.find('#game-0-health-0').element as HTMLInputElement).value).toBe('8')
    expect((wrapper.find('#ban-hero-0').element as HTMLSelectElement).value).toBe('11')
    expect((wrapper.find('#ban-type-0').element as HTMLSelectElement).value).toBe('OPPONENT_BAN')

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(correctMatch).toHaveBeenCalledWith(
      1,
      42,
      expect.objectContaining({
        externalLink: 'https://example.com/bracket/9',
        participants: [{ playerLabel: 'Alice' }, { playerLabel: '' }],
        games: [
          expect.objectContaining({
            gameNumber: 1,
            mapId: 5,
            participants: [
              expect.objectContaining({ heroId: 10, healthRemaining: 8, isWinner: true }),
              expect.objectContaining({ heroId: 11, healthRemaining: 0, isWinner: false }),
            ],
          }),
        ],
        bans: [{ heroId: 11, banType: 'OPPONENT_BAN' }],
      }),
    )
  })
})
