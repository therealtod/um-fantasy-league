import { describe, expect, it } from 'vitest'
import type { Hero, MatchImportPreviewDto, MatchResultDto } from '@/api/types'
import * as matchForm from './matchForm'
import type { MatchForm } from './matchForm'

const heroPool: Hero[] = [
  { id: 10, name: 'Sherlock Holmes', imageUrl: null, cost: 500 },
  { id: 11, name: 'Dracula', imageUrl: null, cost: 400 },
  { id: 12, name: 'Medusa', imageUrl: null, cost: 300 },
  { id: 13, name: 'Bigfoot', imageUrl: null, cost: 200 },
]

const names = (heroes: Hero[]) => heroes.map((hero) => hero.name)

/**
 * A minimal saveable form: two sides, one hero each, one decided game. Every
 * test below starts here and breaks exactly one thing, so a failure names the
 * rule rather than the fixture.
 */
function validForm(overrides: Partial<MatchForm> = {}): MatchForm {
  return {
    ...matchForm.blankForm(),
    externalLink: 'https://example.com/match/1',
    sides: [
      { playerLabel: 'Alice', draftedHeroIds: [10], bans: [] },
      { playerLabel: 'Bob', draftedHeroIds: [11], bans: [] },
    ],
    games: [
      {
        gameNumber: 1,
        mapId: 5,
        participants: [
          { heroId: 10, healthRemaining: 8, isWinner: true },
          { heroId: 11, healthRemaining: 0, isWinner: false },
        ],
      },
    ],
    ...overrides,
  }
}

describe('blankForm', () => {
  it('starts on round 1 with two empty sides and one game', () => {
    const form = matchForm.blankForm()

    expect(form.round).toBe(1)
    expect(form.sides).toHaveLength(2)
    expect(form.sides.every((side) => side.draftedHeroIds.length === 0)).toBe(true)
    expect(form.games).toHaveLength(1)
    // 0 is the "nothing selected" sentinel every dropdown starts on.
    expect(form.games[0]!.mapId).toBe(0)
    expect(form.games[0]!.participants.map((p) => p.heroId)).toEqual([0, 0])
  })
})

/* -------------------------------------------------------------------------
 * The union/subtraction invariant: the form holds each side's whole arsenal,
 * the API holds picks and bans disjoint. `formFromPreview` and `formFromMatch`
 * union; `toPayload` subtracts. These three have to stay in step, so they are
 * tested against each other as well as on their own.
 * ------------------------------------------------------------------------- */

describe('formFromPreview', () => {
  const preview: MatchImportPreviewDto = {
    sourceUrl: 'https://www.tabletopleague.com/o/org/comp/matches/abcdef12',
    roundName: 'The Wayward Sisters',
    seriesFormat: 'BO1',
    playedAt: '2026-08-17T20:00:00Z',
    playedAtRaw: '17 Aug 2026, 22:00 CEST',
    participants: [
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
    bans: [
      { heroId: 12, heroName: 'Medusa', banType: 'OPPONENT_BAN', side: 0 },
      { heroId: 13, heroName: 'Bigfoot', banType: 'PRE_BAN' },
    ],
    unresolved: [],
  }

  it("unions each side's bans back onto the draft the preview split them out of", () => {
    const form = matchForm.formFromPreview(preview)

    // Side 1 played Sherlock, benched Dracula and lost Medusa to a ban — one
    // arsenal, which is the list the admin reads off the match card.
    expect(form.sides[0]!.draftedHeroIds).toEqual([10, 11, 12])
    expect(form.sides[0]!.bans).toEqual([{ heroId: 12, banType: 'OPPONENT_BAN' }])
    expect(form.sides[1]!.draftedHeroIds).toEqual([11])
    expect(form.sides[1]!.bans).toEqual([])
  })

  it('keeps a pre-ban off both drafts, since it precedes side assignment', () => {
    const form = matchForm.formFromPreview(preview)

    expect(form.preBans).toEqual([{ heroId: 13 }])
    expect(form.sides.flatMap((side) => side.draftedHeroIds)).not.toContain(13)
  })

  it('takes the source url as the external link and the scraped instant as played-at', () => {
    const form = matchForm.formFromPreview(preview)

    expect(form.externalLink).toBe(preview.sourceUrl)
    expect(form.playedAt).toBe('2026-08-17T20:00:00Z')
  })

  it('leaves the round at the default, since the source names rounds rather than numbering them', () => {
    expect(matchForm.formFromPreview(preview).round).toBe(1)
  })

  it('falls back to now when the source timestamp was unreadable', () => {
    const { playedAt: _dropped, ...rest } = preview
    const before = Date.now()

    const form = matchForm.formFromPreview(rest)

    expect(new Date(form.playedAt).getTime()).toBeGreaterThanOrEqual(before)
  })

  it('leaves an unresolved ban on the placeholder rather than inventing a hero id', () => {
    const form = matchForm.formFromPreview({
      ...preview,
      // The importer could not resolve this name, so it arrives with no id.
      bans: [{ heroName: 'Dr Ellie Sattler', banType: 'SELF_BAN', side: 1 }],
    })

    expect(form.sides[1]!.bans).toEqual([{ heroId: 0, banType: 'SELF_BAN' }])
    // ...and the sentinel does not leak onto the arsenal as a phantom pick.
    expect(form.sides[1]!.draftedHeroIds).toEqual([11])
  })
})

describe('formFromMatch', () => {
  const saved: MatchResultDto = {
    matchId: 42,
    tournamentId: 1,
    round: 2,
    playedAt: '2026-08-01T12:00:00Z',
    externalLink: 'https://example.com/bracket/9',
    participants: [
      // Deliberately out of order: the form indexes sides by list position, so
      // the sort is what keeps side 1 in slot 1.
      { side: 1, draftedHeroes: [{ heroId: 11, heroName: 'Dracula' }] },
      {
        side: 0,
        playerLabel: 'Alice',
        draftedHeroes: [{ heroId: 10, heroName: 'Sherlock Holmes' }, { heroId: 12, heroName: 'Medusa' }],
      },
    ],
    games: [
      {
        gameId: 101,
        gameNumber: 2,
        mapId: 5,
        mapName: 'Center Square',
        participants: [
          { side: 1, heroId: 11, heroName: 'Dracula', healthRemaining: 3, isWinner: true },
          { side: 0, heroId: 10, heroName: 'Sherlock Holmes', healthRemaining: -1, isWinner: false },
        ],
      },
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

  it('unions a saved ban back onto the arsenal it was struck from', () => {
    const form = matchForm.formFromMatch(saved)

    expect(form.sides[0]!.draftedHeroIds).toEqual([10, 12, 13])
    expect(form.sides[0]!.bans).toEqual([{ heroId: 13, banType: 'OPPONENT_BAN' }])
    expect(form.sides[1]!.bans).toEqual([])
  })

  it('orders sides and games by their own numbering, not by the order they arrived', () => {
    const form = matchForm.formFromMatch(saved)

    expect(form.sides.map((side) => side.playerLabel)).toEqual(['Alice', ''])
    expect(form.games.map((game) => game.gameNumber)).toEqual([1, 2])
    expect(form.games[0]!.participants.map((p) => p.healthRemaining)).toEqual([8, 0])
  })

  it('holds a ban recorded before hero_ban.side existed aside rather than guessing one', () => {
    const form = matchForm.formFromMatch({
      ...saved,
      // Jackson omits nulls, so a pre-V7 row arrives with no `side` at all.
      bans: [{ heroId: 13, heroName: 'Bigfoot', banType: 'SELF_BAN' }],
    })

    expect(form.unassignedBans).toEqual([{ heroId: 13, banType: 'SELF_BAN' }])
    expect(form.sides.flatMap((side) => side.bans)).toEqual([])
  })
})

describe('toPayload', () => {
  it("subtracts a side's own bans back off the draft it sends", () => {
    const form = validForm({
      sides: [
        { playerLabel: 'Alice', draftedHeroIds: [10, 12], bans: [{ heroId: 12, banType: 'OPPONENT_BAN' }] },
        { playerLabel: 'Bob', draftedHeroIds: [11], bans: [] },
      ],
    })

    const payload = matchForm.toPayload(form)

    // Medusa is on the ban list carrying its side, and off the draft —
    // BANNED_HERO_DRAFTED keeps the two disjoint.
    expect(payload.participants[0]!.draftedHeroIds).toEqual([10])
    expect(payload.bans).toEqual([{ heroId: 12, banType: 'OPPONENT_BAN', side: 0 }])
  })

  it('sends a pre-ban with a null side, since it precedes side assignment', () => {
    const payload = matchForm.toPayload(validForm({ preBans: [{ heroId: 13 }] }))

    expect(payload.bans).toEqual([{ heroId: 13, banType: 'PRE_BAN', side: null }])
  })

  it('keeps a drafted-but-unfielded hero on the draft, which is what earns an appearance', () => {
    const form = validForm({
      sides: [
        { playerLabel: 'Alice', draftedHeroIds: [10, 12], bans: [] },
        { playerLabel: 'Bob', draftedHeroIds: [11], bans: [] },
      ],
    })

    expect(matchForm.toPayload(form).participants[0]!.draftedHeroIds).toEqual([10, 12])
  })

  it('trims the external link, so trailing whitespace never becomes a second identity', () => {
    const payload = matchForm.toPayload(validForm({ externalLink: '  https://example.com/match/1  ' }))

    expect(payload.externalLink).toBe('https://example.com/match/1')
  })

  it('names a side by list position, so both sides can carry bans at once', () => {
    const form = validForm({
      sides: [
        { playerLabel: 'Alice', draftedHeroIds: [10, 12], bans: [{ heroId: 12, banType: 'OPPONENT_BAN' }] },
        { playerLabel: 'Bob', draftedHeroIds: [11, 13], bans: [{ heroId: 13, banType: 'SELF_BAN' }] },
      ],
    })

    expect(matchForm.toPayload(form).bans).toEqual([
      { heroId: 12, banType: 'OPPONENT_BAN', side: 0 },
      { heroId: 13, banType: 'SELF_BAN', side: 1 },
    ])
  })
})

/**
 * The three functions above are one invariant read in two directions, so the
 * cheapest guard against them drifting apart is to run a shape through both.
 */
describe('the union and the subtraction are inverses', () => {
  it('round-trips a saved match back to the payload that would rewrite it', () => {
    const saved: MatchResultDto = {
      matchId: 42,
      tournamentId: 1,
      round: 2,
      playedAt: '2026-08-01T12:00:00Z',
      externalLink: 'https://example.com/bracket/9',
      participants: [
        {
          side: 0,
          playerLabel: 'Alice',
          draftedHeroes: [{ heroId: 10, heroName: 'Sherlock Holmes' }, { heroId: 12, heroName: 'Medusa' }],
        },
        { side: 1, playerLabel: 'Bob', draftedHeroes: [{ heroId: 11, heroName: 'Dracula' }] },
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

    const payload = matchForm.toPayload(matchForm.formFromMatch(saved))

    expect(payload.participants).toEqual([
      { playerLabel: 'Alice', draftedHeroIds: [10, 12] },
      { playerLabel: 'Bob', draftedHeroIds: [11] },
    ])
    expect(payload.bans).toEqual([{ heroId: 13, banType: 'OPPONENT_BAN', side: 0 }])
    expect(payload.externalLink).toBe(saved.externalLink)
    expect(payload.round).toBe(saved.round)
  })

  it("round-trips a preview back to the preview's own split, losing nothing", () => {
    const preview: MatchImportPreviewDto = {
      sourceUrl: 'https://www.tabletopleague.com/o/org/comp/matches/abcdef12',
      participants: [
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
      bans: [
        { heroId: 12, heroName: 'Medusa', banType: 'OPPONENT_BAN', side: 0 },
        { heroId: 13, heroName: 'Bigfoot', banType: 'PRE_BAN' },
      ],
      unresolved: [],
    }

    const payload = matchForm.toPayload(matchForm.formFromPreview(preview))

    expect(payload.participants[0]!.draftedHeroIds).toEqual([10, 11])
    expect(payload.participants[1]!.draftedHeroIds).toEqual([11])
    expect(payload.bans).toEqual([
      { heroId: 12, banType: 'OPPONENT_BAN', side: 0 },
      { heroId: 13, banType: 'PRE_BAN', side: null },
    ])
  })
})

/* -------------------------------------------------------------------------
 * Option lists — what makes most of MatchResultPolicy unreachable from the form.
 * ------------------------------------------------------------------------- */

describe('option lists', () => {
  const drafted = validForm({
    sides: [
      { playerLabel: '', draftedHeroIds: [10, 12], bans: [{ heroId: 12, banType: 'OPPONENT_BAN' }] },
      { playerLabel: '', draftedHeroIds: [11], bans: [] },
    ],
  })

  it('offers a game only the heroes that side drafted and did not lose to a ban', () => {
    // Sherlock only: Medusa was struck, Dracula belongs to the other side, and
    // Bigfoot to nobody. This is why PLAYED_HERO_NOT_DRAFTED cannot be built here.
    expect(names(matchForm.fieldableHeroes(drafted, heroPool, 0))).toEqual(['Sherlock Holmes'])
    expect(names(matchForm.fieldableHeroes(drafted, heroPool, 1))).toEqual(['Dracula'])
  })

  it('drops a pre-banned hero from every side that could otherwise field it', () => {
    const withPreBan = { ...drafted, preBans: [{ heroId: 10 }] }

    expect(matchForm.fieldableHeroes(withPreBan, heroPool, 0)).toEqual([])
  })

  it("offers a ban row this side's arsenal, minus what it already struck elsewhere", () => {
    // Medusa is still offered on its *own* row — otherwise the select would
    // render blank while holding the id.
    expect(names(matchForm.bannableHeroes(drafted, heroPool, 0, 12))).toEqual(['Sherlock Holmes', 'Medusa'])
    // ...but not on a second, empty row.
    expect(names(matchForm.bannableHeroes(drafted, heroPool, 0, 0))).toEqual(['Sherlock Holmes'])
  })

  it('offers a pre-ban only heroes neither side drafted', () => {
    expect(names(matchForm.preBannableHeroes(drafted, heroPool, 0))).toEqual(['Bigfoot'])
  })

  it('offers the draft everything not already on that side, its own row included', () => {
    expect(names(matchForm.draftableHeroes(drafted, heroPool, 0, 0))).toEqual(['Dracula', 'Bigfoot'])
    expect(names(matchForm.draftableHeroes(drafted, heroPool, 0, 10))).toEqual([
      'Sherlock Holmes',
      'Dracula',
      'Bigfoot',
    ])
  })

  it('names a hero not in the pool by id rather than rendering nothing', () => {
    expect(matchForm.heroName(heroPool, 10)).toBe('Sherlock Holmes')
    expect(matchForm.heroName(heroPool, 99)).toBe('#99')
  })
})

/* -------------------------------------------------------------------------
 * Edits with a consequence.
 * ------------------------------------------------------------------------- */

describe('removeGame', () => {
  it('renumbers what is left, since game numbers are a dense 1..N sequence', () => {
    const form = validForm({
      games: [1, 2, 3].map((n) => matchForm.blankGame(n)),
    })

    matchForm.removeGame(form, 0)

    expect(form.games.map((game) => game.gameNumber)).toEqual([1, 2])
  })
})

describe('removeDraftPick', () => {
  it('clears the games and bans that named the hero, not just the draft row', () => {
    const form = validForm({
      sides: [
        { playerLabel: '', draftedHeroIds: [10, 12], bans: [{ heroId: 12, banType: 'OPPONENT_BAN' }] },
        { playerLabel: '', draftedHeroIds: [11], bans: [] },
      ],
      games: [
        {
          gameNumber: 1,
          mapId: 5,
          participants: [
            { heroId: 12, healthRemaining: 8, isWinner: true },
            { heroId: 11, healthRemaining: 0, isWinner: false },
          ],
        },
      ],
    })

    matchForm.removeDraftPick(form, 0, 1)

    // Without the cascade the game would keep an id no longer in its own option
    // list: blank on screen, still submitted, and a PLAYED_HERO_NOT_DRAFTED 422.
    expect(form.sides[0]!.draftedHeroIds).toEqual([10])
    expect(form.sides[0]!.bans).toEqual([])
    expect(form.games[0]!.participants[0]!.heroId).toBe(0)
    // The other side's hero is untouched — the cascade is scoped to one draft.
    expect(form.games[0]!.participants[1]!.heroId).toBe(11)
  })

  it('leaves the rest alone when the removed row was still on the placeholder', () => {
    const form = validForm({
      sides: [
        { playerLabel: '', draftedHeroIds: [10, 0], bans: [] },
        { playerLabel: '', draftedHeroIds: [11], bans: [] },
      ],
    })

    matchForm.removeDraftPick(form, 0, 1)

    expect(form.sides[0]!.draftedHeroIds).toEqual([10])
    expect(form.games[0]!.participants[0]!.heroId).toBe(10)
  })
})

describe('assignBanToSide', () => {
  it("puts the ban on the side's list and its hero on that side's arsenal", () => {
    const form = validForm({ unassignedBans: [{ heroId: 13, banType: 'SELF_BAN' }] })

    matchForm.assignBanToSide(form, 0, 0)

    expect(form.unassignedBans).toEqual([])
    expect(form.sides[0]!.bans).toEqual([{ heroId: 13, banType: 'SELF_BAN' }])
    // A ban is struck out of a draft, so its hero has to be on that draft, or
    // the row it just became would offer nothing and read as unfilled.
    expect(form.sides[0]!.draftedHeroIds).toEqual([10, 13])
  })

  it('does not duplicate a hero already on that arsenal', () => {
    const form = validForm({ unassignedBans: [{ heroId: 10, banType: 'SELF_BAN' }] })

    matchForm.assignBanToSide(form, 0, 0)

    expect(form.sides[0]!.draftedHeroIds).toEqual([10])
  })
})

describe('setWinner', () => {
  it('moves the win to the chosen side instead of ever having two', () => {
    const form = validForm()

    matchForm.setWinner(form, 0, 1)

    expect(form.games[0]!.participants.map((p) => p.isWinner)).toEqual([false, true])
  })
})

/* -------------------------------------------------------------------------
 * Validation. One message at a time, in the order the admin meets them.
 * ------------------------------------------------------------------------- */

describe('validate', () => {
  const check = (form: MatchForm) => matchForm.validate(form, heroPool)

  it('passes a complete, decided series', () => {
    expect(check(validForm())).toBeNull()
  })

  it('requires an external link, the thing that stops a double-record', () => {
    expect(check(validForm({ externalLink: '   ' }))).toContain('needs an external link')
  })

  it('blocks on a ban with no side rather than dropping one that already scored', () => {
    const form = validForm({ unassignedBans: [{ heroId: 13, banType: 'SELF_BAN' }] })

    expect(check(form)).toContain("Assign Bigfoot's ban to the side it was struck from")
  })

  it('names the side whose draft row is still on the placeholder', () => {
    const form = validForm()
    form.sides[1]!.draftedHeroIds.push(0)

    expect(check(form)).toBe("Every hero on side 2's draft needs to be chosen")
  })

  it('catches a hero drafted twice by one side (DUPLICATE_PICK)', () => {
    const form = validForm()
    form.sides[0]!.draftedHeroIds.push(10)

    expect(check(form)).toBe('Side 1 drafted Sherlock Holmes twice — remove the duplicate row')
  })

  it('lets both sides draft the same hero, which is not a duplicate pick', () => {
    const form = validForm()
    form.sides[1]!.draftedHeroIds.push(10)

    expect(check(form)).toBeNull()
  })

  it('catches a hero struck twice across the two drafts (DUPLICATE_BAN)', () => {
    // Spans three lists, so no single dropdown can prevent it.
    const form = validForm({
      sides: [
        { playerLabel: '', draftedHeroIds: [10, 13], bans: [{ heroId: 13, banType: 'SELF_BAN' }] },
        { playerLabel: '', draftedHeroIds: [11, 13], bans: [{ heroId: 13, banType: 'OPPONENT_BAN' }] },
      ],
    })

    expect(check(form)).toBe('Bigfoot is banned twice — a hero can only be struck once per series')
  })

  it('catches a hero struck once per draft and once before them', () => {
    const form = validForm({
      sides: [
        { playerLabel: '', draftedHeroIds: [10, 13], bans: [{ heroId: 13, banType: 'SELF_BAN' }] },
        { playerLabel: '', draftedHeroIds: [11], bans: [] },
      ],
      preBans: [{ heroId: 13 }],
    })

    expect(check(form)).toContain('Bigfoot is banned twice')
  })

  it('catches a pre-ban on a hero someone drafted afterwards (BANNED_HERO_DRAFTED)', () => {
    // The dropdown never offers a drafted hero, but a hero drafted *after* the
    // pre-ban was set still gets there — which is why the check spans both lists.
    const form = validForm({ preBans: [{ heroId: 10 }] })

    expect(check(form)).toBe(
      'Sherlock Holmes is pre-banned, so neither side can have drafted it — a pre-ban precedes the draft',
    )
  })

  it('requires a map on every game', () => {
    const form = validForm()
    form.games[0]!.mapId = 0

    expect(check(form)).toBe('Every game needs a map selected')
  })

  it('requires a hero for both sides of every game', () => {
    const form = validForm()
    form.games[0]!.participants[1]!.heroId = 0

    expect(check(form)).toBe('Every game needs a hero selected for both sides')
  })

  it('backstops a fielded hero that is no longer on its draft', () => {
    // Unreachable through the form — the dropdowns cannot offer one — so this
    // fires only if the `removeDraftPick` cascade ever missed a game.
    const form = validForm()
    form.sides[0]!.draftedHeroIds = [12]
    form.games[0]!.participants[0]!.heroId = 10

    expect(check(form)).toBe(
      'Side 1 fields Sherlock Holmes, which is not on its draft — add it or pick another hero',
    )
  })

  it('refuses a game with no winner rather than posting a draw', () => {
    const form = validForm()
    form.games[0]!.participants[0]!.isWinner = false

    expect(check(form)).toBe('Every game needs exactly one winner — a game cannot end in a draw')
  })

  it('refuses a game with two winners', () => {
    const form = validForm()
    form.games[0]!.participants[1]!.isWinner = true

    expect(check(form)).toContain('exactly one winner')
  })

  it('refuses a loser who survived (LOSER_HAS_POSITIVE_HEALTH)', () => {
    const form = validForm()
    form.games[0]!.participants[1]!.healthRemaining = 1

    expect(check(form)).toBe('The losing hero must have 0 or less health')
  })

  it('allows an overkill hit that put the loser below zero', () => {
    const form = validForm()
    form.games[0]!.participants[1]!.healthRemaining = -3

    expect(check(form)).toBeNull()
  })

  it('does not require a player name — an unattributed result is still a result', () => {
    const form = validForm()
    form.sides.forEach((side) => {
      side.playerLabel = ''
    })

    expect(check(form)).toBeNull()
  })

  it('accepts a best-of-N where a side pilots the same hero in more than one game', () => {
    const form = validForm()
    form.games.push({
      gameNumber: 2,
      mapId: 5,
      participants: [
        { heroId: 10, healthRemaining: 0, isWinner: false },
        { heroId: 11, healthRemaining: 4, isWinner: true },
      ],
    })

    // Repeating a hero across games is the ordinary case: the draft names it
    // once and both games draw from it.
    expect(check(form)).toBeNull()
  })
})
