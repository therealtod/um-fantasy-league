import type {
  BanType,
  Hero,
  MatchGameRequest,
  MatchImportPreviewDto,
  MatchResultDto,
  RecordMatchRequest,
} from '@/api/types'

/**
 * The match wizard's form model, and every rule that operates on it.
 *
 * This is `MatchResultWizard.vue`'s domain half, split out for the same reason
 * `rosterPolicy.ts` is: it is data-in/data-out, so it is worth testing as data
 * rather than by driving a DOM. The component keeps the rendering, the reactive
 * `form` ref, and the API calls; everything here is a plain function over plain
 * objects with no Vue import.
 *
 * The shape of that split is one invariant repeated three ways. The *form*
 * holds each side's whole arsenal — the heroes it fielded, benched, and lost to
 * a ban, in one list — because that is the list an admin reads off a match
 * card. The *API* holds picks and bans disjoint (`BANNED_HERO_DRAFTED`). So
 * `toPayload` subtracts the bans back out, and `formFromMatch` /
 * `formFromPreview` union them back in. Change one and change all three.
 */

/** A ban struck out of one side's draft. A `PRE_BAN` belongs to neither and lives in `preBans`. */
export type SidedBanType = Extract<BanType, 'OPPONENT_BAN' | 'SELF_BAN'>

/**
 * One side of the series, as the form holds it.
 *
 * `draftedHeroIds` is that side's *whole* arsenal — every hero it took, whether
 * it went on to field the hero, leave it on the bench, or lose it to a ban. That
 * is deliberately a superset of what the API's own `draftedHeroIds` carries,
 * which excludes bans; `toPayload` subtracts them back out at save time. Holding
 * the superset here is the whole point of the screen: it is what every hero
 * dropdown on the form filters down to.
 */
export interface SideForm {
  playerLabel: string
  draftedHeroIds: number[]
  /** Struck out of *this* side's draft. Every `heroId` here is also on `draftedHeroIds`. */
  bans: { heroId: number; banType: SidedBanType }[]
}

export interface MatchForm {
  round: number
  playedAt: string
  externalLink: string
  sides: SideForm[]
  games: MatchGameRequest[]
  /** Struck before sides were known, so on neither draft. */
  preBans: { heroId: number }[]
  /**
   * Typed bans that came back with no side — every `hero_ban` row written before
   * V7 added the column looks like this. The server tolerates them
   * (`BAN_SIDE_INVALID` polices an impossible side, never a missing one), but
   * the form cannot show a ban it cannot place, so it asks the admin to assign
   * one before saving rather than silently dropping or guessing it.
   */
  unassignedBans: { heroId: number; banType: SidedBanType }[]
}

/** The form's "nothing selected yet" hero and map id — no real row ever has it. */
const UNSET = 0

export function blankGame(gameNumber: number): MatchGameRequest {
  return {
    gameNumber,
    mapId: UNSET,
    participants: [
      { heroId: UNSET, healthRemaining: 0, isWinner: false },
      { heroId: UNSET, healthRemaining: 0, isWinner: false },
    ],
  }
}

export function blankSide(): SideForm {
  return { playerLabel: '', draftedHeroIds: [], bans: [] }
}

export function blankForm(): MatchForm {
  return {
    round: 1,
    playedAt: new Date().toISOString(),
    externalLink: '',
    sides: [blankSide(), blankSide()],
    games: [blankGame(1)],
    preBans: [],
    unassignedBans: [],
  }
}

/* -------------------------------------------------------------------------
 * Seeding: API shape in, form shape out.
 * ------------------------------------------------------------------------- */

/**
 * Seeds the form from an import preview.
 *
 * The preview splits a side's arsenal the way the *API* does — `draftedHeroIds`
 * without the banned heroes, and the bans in their own list carrying the side
 * they came out of. The form holds the two together, so the seeding here is a
 * union rather than the subtraction the old partial-draft model needed.
 *
 * `round` is never in a preview — the source site names its rounds rather than
 * numbering them — so it keeps the blank form's default for the admin to set.
 * `playedAt` falls back the same way when the source's timezone was unreadable.
 */
export function formFromPreview(preview: MatchImportPreviewDto): MatchForm {
  const blank = blankForm()

  return {
    round: blank.round,
    playedAt: preview.playedAt ?? blank.playedAt,
    externalLink: preview.sourceUrl,
    sides: [0, 1].map((side) => {
      const participant = preview.participants[side]
      // UNSET is the form's "nothing selected" sentinel, so a ban the importer
      // could not resolve becomes an unfilled row rather than a broken option.
      const bans = preview.bans
        .filter((ban) => ban.banType !== 'PRE_BAN' && ban.side === side)
        .map((ban) => ({ heroId: ban.heroId ?? UNSET, banType: ban.banType as SidedBanType }))
      return {
        playerLabel: participant?.playerLabel ?? '',
        draftedHeroIds: [
          ...(participant?.draftedHeroIds ?? []),
          ...bans.map((ban) => ban.heroId).filter((heroId) => heroId !== UNSET),
        ],
        bans,
      }
    }),
    games: preview.games.map((game) => ({
      gameNumber: game.gameNumber,
      mapId: game.mapId ?? UNSET,
      participants: [0, 1].map((side) => ({
        heroId: game.participants[side]?.heroId ?? UNSET,
        healthRemaining: game.participants[side]?.healthRemaining ?? 0,
        isWinner: game.participants[side]?.isWinner ?? false,
      })),
    })),
    preBans: preview.bans
      .filter((ban) => ban.banType === 'PRE_BAN')
      .map((ban) => ({ heroId: ban.heroId ?? UNSET })),
    // The importer always attributes a typed ban, since the source groups them
    // under the side that owned the hero — so this is empty in practice and
    // exists only so an unsided one could never vanish silently.
    unassignedBans: preview.bans
      .filter((ban) => ban.banType !== 'PRE_BAN' && ban.side === undefined)
      .map((ban) => ({ heroId: ban.heroId ?? UNSET, banType: ban.banType as SidedBanType })),
  }
}

/** Reads a saved match back into the form, the same union `formFromPreview` does. */
export function formFromMatch(matchData: MatchResultDto): MatchForm {
  const sidedBans = (side: number) =>
    matchData.bans
      .filter((ban) => ban.banType !== 'PRE_BAN' && ban.side === side)
      .map((ban) => ({ heroId: ban.heroId, banType: ban.banType as SidedBanType }))

  return {
    round: matchData.round,
    playedAt: matchData.playedAt,
    externalLink: matchData.externalLink,
    sides: [...matchData.participants]
      .sort((a, b) => a.side - b.side)
      .map((participant) => {
        const bans = sidedBans(participant.side)
        return {
          playerLabel: participant.playerLabel ?? '',
          // The stored draft plus the heroes banned out of it — together, the
          // arsenal this side actually brought.
          draftedHeroIds: [
            ...participant.draftedHeroes.map((hero) => hero.heroId),
            ...bans.map((ban) => ban.heroId),
          ],
          bans,
        }
      }),
    games: [...matchData.games]
      .sort((a, b) => a.gameNumber - b.gameNumber)
      .map((game) => ({
        gameNumber: game.gameNumber,
        mapId: game.mapId,
        participants: [...game.participants]
          .sort((a, b) => a.side - b.side)
          .map((p) => ({ heroId: p.heroId, healthRemaining: p.healthRemaining, isWinner: p.isWinner })),
      })),
    preBans: matchData.bans.filter((ban) => ban.banType === 'PRE_BAN').map((ban) => ({ heroId: ban.heroId })),
    unassignedBans: matchData.bans
      .filter((ban) => ban.banType !== 'PRE_BAN' && ban.side === undefined)
      .map((ban) => ({ heroId: ban.heroId, banType: ban.banType as SidedBanType })),
  }
}

/* -------------------------------------------------------------------------
 * Saving: form shape in, API shape out.
 * ------------------------------------------------------------------------- */

/**
 * The form's shape, translated to the API's.
 *
 * The one real conversion is the draft: the form holds a side's whole arsenal,
 * the API wants it without the banned heroes, since `BANNED_HERO_DRAFTED` keeps
 * picks and bans disjoint. Subtracting here rather than making the admin keep
 * two lists in their head is the point of the screen.
 */
export function toPayload(form: MatchForm): RecordMatchRequest {
  return {
    round: form.round,
    playedAt: form.playedAt,
    externalLink: form.externalLink.trim(),
    participants: form.sides.map((side) => ({
      playerLabel: side.playerLabel,
      draftedHeroIds: side.draftedHeroIds.filter(
        (heroId) => !side.bans.some((ban) => ban.heroId === heroId),
      ),
    })),
    games: form.games,
    bans: [
      ...form.sides.flatMap((sideForm, side) =>
        sideForm.bans.map((ban) => ({ heroId: ban.heroId, banType: ban.banType, side })),
      ),
      // A pre-ban is struck before sides exist, so it carries none — the server
      // rejects one that does (`BAN_SIDE_INVALID`).
      ...form.preBans.map((ban) => ({ heroId: ban.heroId, banType: 'PRE_BAN' as const, side: null })),
    ],
  }
}

/* -------------------------------------------------------------------------
 * Option lists. Each answers "what may this dropdown offer?", and together they
 * are why most of `match_policy`'s `MatchRule` violations are unreachable from
 * the form at all.
 * ------------------------------------------------------------------------- */

/** The heroes a side fielded, read off the form — a side is the participant's list position. */
export function fieldedInForm(form: MatchForm, side: number): number[] {
  return form.games.flatMap((game) => {
    const participant = game.participants[side]
    return participant && participant.heroId !== UNSET ? [participant.heroId] : []
  })
}

/** The heroes this side lost to a ban — off its arsenal, and so off every dropdown below. */
export function bannedBySide(form: MatchForm, side: number): number[] {
  return form.sides[side]?.bans.map((ban) => ban.heroId) ?? []
}

export function preBannedHeroIds(form: MatchForm): number[] {
  return form.preBans.map((ban) => ban.heroId)
}

/**
 * What a side can actually field: its arsenal, less anything struck out of it.
 * This is the option list for that side's per-game hero dropdowns, and the
 * reason `PLAYED_HERO_NOT_DRAFTED` cannot fire on a submission this form built.
 */
export function fieldableHeroes(form: MatchForm, heroPool: Hero[], side: number): Hero[] {
  const drafted = form.sides[side]?.draftedHeroIds ?? []
  const struck = new Set([...bannedBySide(form, side), ...preBannedHeroIds(form)])
  return heroPool.filter((hero) => drafted.includes(hero.id) && !struck.has(hero.id))
}

/** A side's own ban options: its arsenal, less the heroes it has already struck on another row. */
export function bannableHeroes(
  form: MatchForm,
  heroPool: Hero[],
  side: number,
  currentHeroId: number,
): Hero[] {
  const drafted = form.sides[side]?.draftedHeroIds ?? []
  const alreadyStruck = bannedBySide(form, side).filter((heroId) => heroId !== currentHeroId)
  return heroPool.filter((hero) => drafted.includes(hero.id) && !alreadyStruck.includes(hero.id))
}

/**
 * A pre-ban precedes the draft, so it can only name a hero neither side took.
 * Offering the drafted ones would be offering a `BANNED_HERO_DRAFTED` 422.
 */
export function preBannableHeroes(form: MatchForm, heroPool: Hero[], currentHeroId: number): Hero[] {
  const drafted = new Set(form.sides.flatMap((side) => side.draftedHeroIds))
  const alreadyStruck = preBannedHeroIds(form).filter((heroId) => heroId !== currentHeroId)
  return heroPool.filter((hero) => !drafted.has(hero.id) && !alreadyStruck.includes(hero.id))
}

/** Heroes not yet on this side's arsenal — everything is draftable until it is drafted. */
export function draftableHeroes(
  form: MatchForm,
  heroPool: Hero[],
  side: number,
  currentHeroId: number,
): Hero[] {
  const alreadyDrafted = (form.sides[side]?.draftedHeroIds ?? []).filter(
    (heroId) => heroId !== currentHeroId,
  )
  return heroPool.filter((hero) => !alreadyDrafted.includes(hero.id))
}

export function heroName(heroPool: Hero[], heroId: number): string {
  return heroPool.find((hero) => hero.id === heroId)?.name ?? `#${heroId}`
}

/* -------------------------------------------------------------------------
 * Edits with a consequence. The plain pushes and splices stay in the
 * component; these four each carry an invariant that has to survive them.
 * ------------------------------------------------------------------------- */

/** Game numbers are always a dense 1..N sequence, so a removal renumbers the rest. */
export function removeGame(form: MatchForm, index: number) {
  form.games.splice(index, 1)
  form.games.forEach((game, i) => {
    game.gameNumber = i + 1
  })
}

/**
 * Dropping a hero off a side's arsenal drops it everywhere that arsenal fed.
 *
 * Without the cascade a game select would keep a value no longer in its own
 * option list — the browser renders that as blank while the model still holds
 * the id, so the form would look filled and submit a hero the side never
 * drafted.
 */
export function removeDraftPick(form: MatchForm, side: number, index: number) {
  const [heroId] = form.sides[side].draftedHeroIds.splice(index, 1)
  if (heroId === UNSET) return

  form.sides[side].bans = form.sides[side].bans.filter((ban) => ban.heroId !== heroId)
  form.games.forEach((game) => {
    const participant = game.participants[side]
    if (participant?.heroId === heroId) participant.heroId = UNSET
  })
}

/** Moves an unsided ban onto the side whose draft it came out of. */
export function assignBanToSide(form: MatchForm, index: number, side: number) {
  const [ban] = form.unassignedBans.splice(index, 1)
  form.sides[side].bans.push(ban)
  // The ban's hero has to be on the arsenal it was struck from, or the ban row
  // it just became would offer nothing and read as unfilled.
  if (ban.heroId !== UNSET && !form.sides[side].draftedHeroIds.includes(ban.heroId)) {
    form.sides[side].draftedHeroIds.push(ban.heroId)
  }
}

/**
 * Picking a side makes it the sole winner of that game and clears the other.
 * There is no way to pick *neither*, and that is deliberate: every game is
 * played to a decision, so the server rejects a game with no winner
 * (NOT_EXACTLY_ONE_WINNER). The losing side must finish with 0 or less health;
 * the winning side may have any health, including a negative value.
 */
export function setWinner(form: MatchForm, gameIndex: number, participantIndex: number) {
  form.games[gameIndex].participants.forEach((p, i) => {
    p.isWinner = i === participantIndex
  })
}

/* -------------------------------------------------------------------------
 * Validation.
 * ------------------------------------------------------------------------- */

/**
 * Everything the form can rule out on its own, as one message or null.
 *
 * The component calls this from a computed rather than as a step inside its
 * save on purpose: an admin who fixes the problem in place sees the banner
 * clear as they type, instead of a stale complaint that survives until the next
 * save.
 *
 * Most of what the old partial-draft form had to police is now unrepresentable:
 * a game dropdown only offers heroes that side drafted and did not lose to a
 * ban, so `PLAYED_HERO_NOT_DRAFTED` and `BANNED_HERO_PLAYED` cannot be built
 * here at all. What remains is either a half-filled row or a conflict the
 * dropdowns cannot see, because it spans two of them.
 */
export function validate(form: MatchForm, heroPool: Hero[]): string | null {
  const { games, sides, preBans, unassignedBans } = form
  const named = (heroId: number) => heroName(heroPool, heroId)

  // Required server-side, and the duplicate check depends on it. Caught here so
  // an untouched box reads as a prompt rather than a 400.
  if (form.externalLink.trim().length === 0) {
    return 'This match needs an external link — it is what stops the same match being recorded twice'
  }

  if (unassignedBans.length > 0) {
    return `Assign ${named(unassignedBans[0].heroId)}'s ban to the side it was struck from — this match predates per-side bans`
  }

  for (const [side, sideForm] of sides.entries()) {
    if (sideForm.draftedHeroIds.some((heroId) => heroId === UNSET)) {
      return `Every hero on side ${side + 1}'s draft needs to be chosen`
    }
    // DUPLICATE_PICK server-side; caught here so it reads as a prompt, not a 422.
    const seen = new Set<number>()
    for (const heroId of sideForm.draftedHeroIds) {
      if (seen.has(heroId)) {
        return `Side ${side + 1} drafted ${named(heroId)} twice — remove the duplicate row`
      }
      seen.add(heroId)
    }
    if (sideForm.bans.some((ban) => ban.heroId === UNSET)) return `Every ban on side ${side + 1} needs a hero selected`
  }

  if (preBans.some((ban) => ban.heroId === UNSET)) return 'Every pre-ban needs a hero selected'

  // DUPLICATE_BAN server-side: `hero_ban` is keyed (match_id, hero_id), so one
  // hero is struck at most once per series however many sides wanted it. Spans
  // three lists, so no single dropdown can prevent it.
  const allBans = [...sides.flatMap((side) => side.bans.map((ban) => ban.heroId)), ...preBans.map((ban) => ban.heroId)]
  const bannedTwice = allBans.find((heroId, index) => heroId !== UNSET && allBans.indexOf(heroId) !== index)
  if (bannedTwice !== undefined) {
    return `${named(bannedTwice)} is banned twice — a hero can only be struck once per series`
  }

  // BANNED_HERO_DRAFTED server-side. A side's own bans come off its draft at
  // save time, but a pre-ban does not, so it must name a hero neither side took.
  const drafted = new Set(sides.flatMap((side) => side.draftedHeroIds))
  const preBannedButDrafted = preBans.find((ban) => ban.heroId !== UNSET && drafted.has(ban.heroId))
  if (preBannedButDrafted) {
    return `${named(preBannedButDrafted.heroId)} is pre-banned, so neither side can have drafted it — a pre-ban precedes the draft`
  }

  if (games.length === 0) return 'At least one game is required'
  if (games.some((g) => g.mapId === UNSET)) return 'Every game needs a map selected'
  // The player name is deliberately not required: it is a free-text label, and an
  // unattributed result is still a valid result.
  if (games.some((g) => g.participants.some((p) => p.heroId === UNSET))) {
    return 'Every game needs a hero selected for both sides'
  }
  // A backstop, not a prompt the admin should ever see: the dropdowns cannot
  // offer an undrafted hero. It fires only if a hero left a draft while a game
  // still named it and the cascade in `removeDraftPick` somehow missed it.
  for (const [side] of sides.entries()) {
    const fieldable = fieldableHeroes(form, heroPool, side).map((hero) => hero.id)
    const stray = fieldedInForm(form, side).find((heroId) => !fieldable.includes(heroId))
    if (stray !== undefined) {
      return `Side ${side + 1} fields ${named(stray)}, which is not on its draft — add it or pick another hero`
    }
  }
  // The server rejects this too (NOT_EXACTLY_ONE_WINNER) — caught here so an
  // untouched winner radio reads as a prompt rather than a 422.
  if (games.some((g) => g.participants.filter((p) => p.isWinner).length !== 1)) {
    return 'Every game needs exactly one winner — a game cannot end in a draw'
  }
  if (games.some((g) => g.participants.some((p) => !p.isWinner && p.healthRemaining > 0))) {
    return 'The losing hero must have 0 or less health'
  }

  return null
}
