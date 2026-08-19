import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Hero, MapAdminDto, MatchImportPreviewDto, MatchResultDto } from '@/api/types'

const listMapPool = vi.fn()
const listHeroPool = vi.fn()
const recordMatch = vi.fn()
const correctMatch = vi.fn()
const getMatch = vi.fn()
const violationMessagesMock = vi.fn((_e: unknown) => [] as string[])

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
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: (e: unknown) => violationMessagesMock(e),
}))

vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({ tournaments: [] }),
}))

const mapPool: MapAdminDto[] = [{ id: 5, name: 'Center Square' }]
const heroPool: Hero[] = [
  { id: 10, name: 'Sherlock Holmes', imageUrl: null, cost: 500 },
  { id: 11, name: 'Dracula', imageUrl: null, cost: 400 },
]

async function mountWizard(props: {
  tournamentId?: number
  matchId?: number
  mode: 'create' | 'edit'
  prefill?: MatchImportPreviewDto | null
}) {
  const MatchResultWizard = (await import('./MatchResultWizard.vue')).default
  const wrapper = mount(MatchResultWizard, { props })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  listMapPool.mockResolvedValue(mapPool)
  listHeroPool.mockResolvedValue(heroPool)
  violationMessagesMock.mockReturnValue([])
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

  it('sends each side a complete draft, folding in the heroes it fielded', async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    await wrapper.find('#game-0-health-1').setValue('0')
    await wrapper.findAll('input[type=radio]')[0]!.setValue()

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    // Nobody typed a draft, and the submission still carries one: a side that
    // fields a hero drafted it, which is what PLAYED_HERO_NOT_DRAFTED demands.
    expect(recordMatch).toHaveBeenCalledWith(
      1,
      expect.objectContaining({
        participants: [
          expect.objectContaining({ draftedHeroIds: [10] }),
          expect.objectContaining({ draftedHeroIds: [11] }),
        ],
      }),
    )
  })

  it('sends a hero drafted and never fielded, which is what earns an appearance', async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    await wrapper.find('#game-0-health-1').setValue('0')
    await wrapper.findAll('input[type=radio]')[0]!.setValue()

    // The first side also drafted Dracula and never brought it to the table.
    await wrapper.findAll('button.btn-ghost').find((b) => b.text() === '+ Add Drafted Hero')!.trigger('click')
    await wrapper.find('#draft-0-0').setValue('11')

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).toHaveBeenCalledWith(
      1,
      expect.objectContaining({
        participants: [
          expect.objectContaining({ draftedHeroIds: [10, 11] }),
          expect.objectContaining({ draftedHeroIds: [11] }),
        ],
      }),
    )
  })

  it('refuses to save a draft row left on the placeholder', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    await wrapper.find('#game-0-health-1').setValue('0')
    await wrapper.findAll('input[type=radio]')[0]!.setValue()
    await wrapper.findAll('button.btn-ghost').find((b) => b.text() === '+ Add Drafted Hero')!.trigger('click')

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Every drafted hero needs a hero selected')
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
        { side: 0, playerLabel: 'Alice', draftedHeroes: [{ heroId: 10, heroName: 'Sherlock Holmes' }, { heroId: 12, heroName: 'Medusa' }] },
        { side: 1, draftedHeroes: [{ heroId: 11, heroName: 'Dracula' }] },
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
        // Side 0's draft round-trips whole: Sherlock Holmes because it was
        // fielded, Medusa because it was drafted and never played.
        participants: [
          { playerLabel: 'Alice', draftedHeroIds: [10, 12] },
          { playerLabel: '', draftedHeroIds: [11] },
        ],
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

  it('renders a server-side 422 as one line per violation instead of a joined sentence', async () => {
    // Mirrors GlobalExceptionHandler's MatchRuleException shape: `describeError` collapses this to
    // one semicolon-joined message, `violationMessages` recovers the structured list underneath it.
    recordMatch.mockRejectedValue(new Error('Exactly one winner is required; The loser must have 0 or less health'))
    violationMessagesMock.mockReturnValue([
      'Exactly one winner is required',
      'The loser must have 0 or less health',
    ])

    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    await wrapper.find('#game-0-health-1').setValue('0')
    await wrapper.findAll('input[type=radio]')[0]!.setValue()

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    const items = wrapper.findAll('li')
    expect(items.map((li) => li.text())).toEqual([
      'Exactly one winner is required',
      'The loser must have 0 or less health',
    ])
  })
})

/**
 * The wizard's `draftedHeroIds` holds only the picks a side never fielded —
 * `draftFor` unions them back with the game heroes at save time. An import
 * preview carries the *complete* draft, so seeding it in raw would show every
 * hero that played a second time as a "drafted, not played" row. These tests pin
 * the subtraction that prevents it.
 */
describe('MatchResultWizard prefilled from an import', () => {
  const preview: MatchImportPreviewDto = {
    sourceUrl: 'https://www.tabletopleague.com/o/org/comp/matches/abcdef12',
    roundName: 'The Wayward Sisters',
    seriesFormat: 'BO1',
    playedAt: '2026-08-17T20:00:00Z',
    playedAtRaw: '17 Aug 2026, 22:00 CEST',
    participants: [
      // Sherlock played; Dracula was drafted by side 0 and never fielded.
      { playerLabel: 'mystic_owl', draftedHeroIds: [10, 11] },
      { playerLabel: 'immortal', draftedHeroIds: [11] },
    ],
    games: [
      {
        gameNumber: 1,
        mapId: 5,
        mapName: 'Center Square',
        participants: [
          { heroId: 10, heroName: 'Sherlock Holmes', healthRemaining: 12, isWinner: true },
          { heroId: 11, heroName: 'Dracula', healthRemaining: 0, isWinner: false },
        ],
      },
    ],
    bans: [],
    unresolved: [],
  }

  it('seeds the games, players and source link from the preview', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create', prefill: preview })

    expect((wrapper.find('#game-0-map').element as HTMLSelectElement).value).toBe('5')
    expect((wrapper.find('#game-0-hero-0').element as HTMLSelectElement).value).toBe('10')
    expect((wrapper.find('#game-0-health-0').element as HTMLInputElement).value).toBe('12')
    expect((wrapper.find('#player-0').element as HTMLInputElement).value).toBe('mystic_owl')
    expect((wrapper.find('#player-1').element as HTMLInputElement).value).toBe('immortal')
  })

  it('shows only the unfielded picks as drafted-not-played, not the whole draft', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create', prefill: preview })

    // Side 0 drafted 10 and 11 but fielded 10, so exactly one editable pick row
    // remains — and it is 11, not 10.
    const side0Picks = wrapper.findAll('[id^="draft-0-"]')
    expect(side0Picks).toHaveLength(1)
    expect((side0Picks[0]!.element as HTMLSelectElement).value).toBe('11')

    // Side 1 drafted only what it played, so it has no leftover picks at all.
    expect(wrapper.findAll('[id^="draft-1-"]')).toHaveLength(0)
  })

  it('submits each side\'s complete draft exactly once', async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create', prefill: preview })

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    const payload = recordMatch.mock.calls[0]![1] as { participants: { draftedHeroIds: number[] }[] }
    // The full draft is restored by draftFor — no hero duplicated by the round trip.
    expect(payload.participants[0]!.draftedHeroIds.slice().sort()).toEqual([10, 11])
    expect(payload.participants[1]!.draftedHeroIds).toEqual([11])
  })

  it('carries the source url through as the external link', async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create', prefill: preview })

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).toHaveBeenCalledWith(
      1,
      expect.objectContaining({ externalLink: preview.sourceUrl }),
    )
  })

  /** The source names rounds rather than numbering them, so the admin still sets it. */
  it('leaves the round at the default for the admin to set', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create', prefill: preview })
    expect((wrapper.find('#match-round').element as HTMLInputElement).value).toBe('1')
  })

  it('falls back to the blank form when no prefill is given', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })
    expect((wrapper.find('#game-0-map').element as HTMLSelectElement).value).toBe('0')
    expect(wrapper.findAll('[id^="draft-0-"]')).toHaveLength(0)
  })
})
