/**
 * The wizard's *wiring*: that the pools reach the dropdowns, that a filled-in
 * form reaches the API, and that a complaint reaches the banner.
 *
 * The rules themselves — the arsenal union, the payload subtraction, the option
 * lists, every validation message — are plain functions in `@/domain/matchForm`
 * and are tested as data in `matchForm.spec.ts`. A new rule belongs there, not
 * in another `mount()`; only add a test here when the thing that can break is
 * the binding between the two.
 *
 * The wizard renders its four sections (`MatchUnassignedBanSection`,
 * `MatchDraftSide`, `MatchGameRow`, `MatchPreBanSection`) as real children, not
 * stubs, and that is deliberate: each section is a template over the same
 * `matchForm` functions, so mounting the tree is what pins the model reaching
 * them and their edits reaching the payload. There is nothing left in one to
 * test on its own.
 */
import { flushPromises, mount } from '@vue/test-utils'
import type { VueWrapper } from '@vue/test-utils'
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
  { id: 12, name: 'Medusa', imageUrl: null, cost: 300 },
  { id: 13, name: 'Bigfoot', imageUrl: null, cost: 200 },
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
  // `external_link` is required on every save and unique per tournament. A
  // create with no prefill starts blank, so give it one here rather than in
  // every test — a prefill or an edit arrives with its own.
  if (props.mode === 'create' && !props.prefill) {
    await wrapper.find('#match-external-link').setValue('https://example.com/match/spec')
  }
  return wrapper
}

/**
 * Fills one side's draft, which every other dropdown on the form is filtered
 * to. Nothing below the draft can be selected before this runs — that is the
 * whole design, so almost every test starts here.
 */
async function draftSide(wrapper: VueWrapper, side: number, heroIds: number[]) {
  for (const heroId of heroIds) {
    // Append, rather than assuming the side started empty — several tests draft
    // in two passes to set a pre-ban up against a hero drafted afterwards.
    const index = wrapper.findAll(`[id^="draft-${side}-"]`).length
    const addButtons = wrapper.findAll('button.btn-ghost').filter((b) => b.text() === '+ Add Drafted Hero')
    await addButtons[side]!.trigger('click')
    await wrapper.find(`#draft-${side}-${index}`).setValue(String(heroId))
  }
}

/** Both sides at once, for the common one-hero-each case. */
async function draftBothSides(wrapper: VueWrapper, side0: number[], side1: number[]) {
  await draftSide(wrapper, 0, side0)
  await draftSide(wrapper, 1, side1)
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

    // The hero pool reaches the *draft* dropdown, which is the only place that
    // offers the whole pool. A game's dropdown offers a side's draft, and there
    // is no draft yet, so it starts out empty but for its prompt.
    await draftSide(wrapper, 0, [])
    await wrapper.findAll('button.btn-ghost').filter((b) => b.text() === '+ Add Drafted Hero')[0]!.trigger('click')
    expect(wrapper.find('#draft-0-0').text()).toContain('Sherlock Holmes')
    expect(wrapper.find('#game-0-hero-0').text()).toContain('Draft heroes for side 1 first')
    expect(wrapper.find('#game-0-hero-0').text()).not.toContain('Sherlock Holmes')
  })

  it('offers a game only the heroes that side drafted, never the whole pool', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await draftBothSides(wrapper, [10, 12], [11])

    // Side 1 drafted Sherlock and Medusa, so those are its only two options —
    // Dracula belongs to the other side and Bigfoot to nobody.
    const side0 = wrapper.find('#game-0-hero-0').text()
    expect(side0).toContain('Sherlock Holmes')
    expect(side0).toContain('Medusa')
    expect(side0).not.toContain('Dracula')
    expect(side0).not.toContain('Bigfoot')

    const side1 = wrapper.find('#game-0-hero-1').text()
    expect(side1).toContain('Dracula')
    expect(side1).not.toContain('Sherlock Holmes')
  })

  it('drops a hero from the games and bans it fed when it leaves the draft', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await draftBothSides(wrapper, [10, 12], [11])
    await wrapper.find('#game-0-hero-0').setValue('12')
    expect((wrapper.find('#game-0-hero-0').element as HTMLSelectElement).value).toBe('12')

    // Removing Medusa from the draft clears the game that fielded it, rather
    // than leaving a select that renders blank while still holding the id.
    await wrapper.findAll('button.btn-ghost').filter((b) => b.text() === 'Remove')[1]!.trigger('click')

    expect((wrapper.find('#game-0-hero-0').element as HTMLSelectElement).value).toBe('0')
    expect(wrapper.find('#game-0-hero-0').text()).not.toContain('Medusa')
  })

  it('maps a filled-in form to RecordMatchRequest on create', async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await draftBothSides(wrapper, [10], [11])
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

  it('drops its own complaint as soon as the form is corrected, with no second save', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await draftBothSides(wrapper, [10], [11])
    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    await wrapper.find('#game-0-health-1').setValue('0')

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()
    // The rule itself is `matchForm.validate`'s, tested as data next door. What
    // this pins is that its message reaches the banner and holds the save.
    expect(recordMatch).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('cannot end in a draw')

    // Fixing it in place clears the banner — the admin never has to guess
    // whether the message still describes the form in front of them.
    await wrapper.findAll('input[type=radio]')[0]!.setValue()
    expect(wrapper.text()).not.toContain('cannot end in a draw')
  })

  it('drops a rejected save\'s message once the admin edits the form', async () => {
    recordMatch.mockRejectedValue(new Error('Hero(es) drafted more than once by one side: 11.'))
    violationMessagesMock.mockReturnValue(['Hero(es) drafted more than once by one side: 11.'])
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await draftBothSides(wrapper, [10], [11])
    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    await wrapper.find('#game-0-health-1').setValue('0')
    await wrapper.findAll('input[type=radio]')[0]!.setValue()

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('drafted more than once')

    // The rejection described a payload that no longer exists.
    await wrapper.find('#match-round').setValue('2')
    await flushPromises()
    expect(wrapper.text()).not.toContain('drafted more than once')
  })

  it("keeps a side's own ban off the draft it submits, since picks and bans are disjoint", async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    // Side 1 brought three heroes and lost Medusa to the opponent's ban.
    await draftBothSides(wrapper, [10, 12], [11])
    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    await wrapper.find('#game-0-health-1').setValue('0')
    await wrapper.findAll('input[type=radio]')[0]!.setValue()

    await wrapper.findAll('button.btn-ghost').filter((b) => b.text() === '+ Add Ban')[0]!.trigger('click')
    await wrapper.find('#ban-hero-0-0').setValue('12')

    // A banned hero also drops out of that side's game dropdown — it never
    // reached the table, so BANNED_HERO_PLAYED cannot be built here either.
    expect(wrapper.find('#game-0-hero-0').text()).not.toContain('Medusa')

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).toHaveBeenCalledWith(
      1,
      expect.objectContaining({
        // Medusa is on the ban list carrying its side, and off the draft.
        participants: [
          expect.objectContaining({ draftedHeroIds: [10] }),
          expect.objectContaining({ draftedHeroIds: [11] }),
        ],
        bans: [{ heroId: 12, banType: 'OPPONENT_BAN', side: 0 }],
      }),
    )
  })

  it('sends a pre-ban with no side, since it precedes side assignment', async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await draftBothSides(wrapper, [10], [11])
    await wrapper.find('#game-0-map').setValue('5')
    await wrapper.find('#game-0-hero-0').setValue('10')
    await wrapper.find('#game-0-hero-1').setValue('11')
    await wrapper.find('#game-0-health-1').setValue('0')
    await wrapper.findAll('input[type=radio]')[0]!.setValue()

    await wrapper.findAll('button.btn-ghost').find((b) => b.text() === '+ Add Pre-ban')!.trigger('click')
    await wrapper.find('#pre-ban-hero-0').setValue('13')

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(recordMatch).toHaveBeenCalledWith(
      1,
      expect.objectContaining({ bans: [{ heroId: 13, banType: 'PRE_BAN', side: null }] }),
    )
  })

  it('moves the winner to the other side instead of ever having two', async () => {
    recordMatch.mockResolvedValue({})
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })

    await draftBothSides(wrapper, [10], [11])
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
      bans: [{ heroId: 13, heroName: 'Bigfoot', banType: 'OPPONENT_BAN', side: 0 }],
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
    // The ban reads back onto the side whose draft it came out of, and the hero
    // is on that side's arsenal — the round trip `hero_ban.side` exists for.
    expect((wrapper.find('#ban-hero-0-0').element as HTMLSelectElement).value).toBe('13')
    expect((wrapper.find('#ban-type-0-0').element as HTMLSelectElement).value).toBe('OPPONENT_BAN')
    expect(wrapper.findAll('[id^="draft-0-"]').map((s) => (s.element as HTMLSelectElement).value))
      .toEqual(['10', '12', '13'])
    expect(wrapper.findAll('[id^="ban-hero-1-"]')).toHaveLength(0)

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(correctMatch).toHaveBeenCalledWith(
      1,
      42,
      expect.objectContaining({
        externalLink: 'https://example.com/bracket/9',
        // Side 0's draft round-trips whole: Sherlock Holmes because it was
        // fielded, Medusa because it was drafted and never played. Bigfoot is
        // on the form's arsenal but back off the wire, since a banned hero is
        // not a pick (BANNED_HERO_DRAFTED).
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
        bans: [{ heroId: 13, banType: 'OPPONENT_BAN', side: 0 }],
      }),
    )
  })

  it('asks for a side on a ban recorded before bans had one, instead of dropping it', async () => {
    getMatch.mockResolvedValue({
      matchId: 43,
      tournamentId: 1,
      round: 1,
      playedAt: '2026-08-01T12:00:00Z',
      externalLink: 'https://example.com/match/43',
      participants: [
        { side: 0, playerLabel: 'Alice', draftedHeroes: [{ heroId: 10, heroName: 'Sherlock Holmes' }] },
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
      // No `side` — every hero_ban row written before V7 looks like this.
      bans: [{ heroId: 13, heroName: 'Bigfoot', banType: 'SELF_BAN' }],
    } as MatchResultDto)

    const wrapper = await mountWizard({ tournamentId: 1, matchId: 43, mode: 'edit' })

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    // It blocks the save rather than guessing a side or silently dropping a ban
    // that already scored points.
    expect(correctMatch).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Bans with no side')

    // Placing it puts the hero on that side's arsenal and lets the save through.
    await wrapper.findAll('button.btn-ghost').find((b) => b.text() === 'Side 1')!.trigger('click')
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(correctMatch).toHaveBeenCalledWith(
      1,
      43,
      expect.objectContaining({ bans: [{ heroId: 13, banType: 'SELF_BAN', side: 0 }] }),
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

    await draftBothSides(wrapper, [10], [11])
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
 * The form holds a side's whole arsenal — what it drafted, plus what it lost to
 * a ban — while the preview splits those the way the API does. These tests pin
 * the union that puts them back together, and the ban sides that survive it.
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
    // Side 1 also brought Medusa and lost it to the opponent's ban; Bigfoot was
    // struck before sides were known, so it belongs to neither draft.
    bans: [
      { heroId: 12, heroName: 'Medusa', banType: 'OPPONENT_BAN', side: 0 },
      { heroId: 13, heroName: 'Bigfoot', banType: 'PRE_BAN' },
    ],
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

  it("shows a side's whole arsenal, banned heroes included", async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create', prefill: preview })

    // Side 1 played Sherlock, benched Dracula and lost Medusa to a ban — three
    // rows, in one list, which is the list the admin reads off the match card.
    expect(wrapper.findAll('[id^="draft-0-"]').map((s) => (s.element as HTMLSelectElement).value))
      .toEqual(['10', '11', '12'])
    expect((wrapper.find('#ban-hero-0-0').element as HTMLSelectElement).value).toBe('12')

    // Side 2 drafted only what it played, and struck nothing.
    expect(wrapper.findAll('[id^="draft-1-"]').map((s) => (s.element as HTMLSelectElement).value))
      .toEqual(['11'])
    expect(wrapper.findAll('[id^="ban-hero-1-"]')).toHaveLength(0)

    // The pre-ban belongs to neither, so it sits in its own section.
    expect((wrapper.find('#pre-ban-hero-0').element as HTMLSelectElement).value).toBe('13')
  })

  it('falls back to the blank form when no prefill is given', async () => {
    const wrapper = await mountWizard({ tournamentId: 1, mode: 'create' })
    expect((wrapper.find('#game-0-map').element as HTMLSelectElement).value).toBe('0')
    expect(wrapper.findAll('[id^="draft-0-"]')).toHaveLength(0)
  })
})
