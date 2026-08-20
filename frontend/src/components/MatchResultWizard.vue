<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { api, describeError, violationMessages } from '@/api/client'
import { useTournamentsStore } from '@/stores/tournaments'
import { byName } from '@/lib/sort'
import type {
  BanType,
  Hero,
  MapAdminDto,
  MatchImportPreviewDto,
  MatchGameRequest,
  MatchResultDto,
  RecordMatchRequest,
} from '@/api/types'
import ErrorBanner from '@/components/ErrorBanner.vue'

interface Props {
  tournamentId?: number
  matchId?: number
  mode: 'create' | 'edit'
  /**
   * A scraped match to seed the form with, from the import panel. Create mode
   * only — an import produces a new match, never an edit of an existing one.
   */
  prefill?: MatchImportPreviewDto | null
}

const props = defineProps<Props>()
const emit = defineEmits<{
  success: []
  cancel: []
}>()

const tournamentsStore = useTournamentsStore()

const loading = ref(false)
// What the *server* (or a failed load) said, as opposed to what this form can
// work out for itself — see `validationError`.
const serverError = ref<string | null>(null)
const violations = ref<string[]>([])
const submitAttempted = ref(false)
const isInitialized = ref(false)

/** A ban struck out of one side's draft. A `PRE_BAN` belongs to neither and lives in `preBans`. */
type SidedBanType = Extract<BanType, 'OPPONENT_BAN' | 'SELF_BAN'>

const sidedBanTypes: { value: SidedBanType; label: string }[] = [
  { value: 'OPPONENT_BAN', label: 'Opponent ban' },
  { value: 'SELF_BAN', label: 'Self ban' },
]

const banTypeLabels: Record<BanType, string> = {
  PRE_BAN: 'Pre-ban',
  OPPONENT_BAN: 'Opponent ban',
  SELF_BAN: 'Self ban',
}

// The tournament's legal boards and priced hero pool — what the match actually
// scores against — so the admin picks from a list instead of typing a raw id.
const mapPool = ref<MapAdminDto[]>([])
const heroPool = ref<Hero[]>([])

/**
 * One side of the series, as this form holds it.
 *
 * `draftedHeroIds` is that side's *whole* arsenal — every hero it took, whether
 * it went on to field the hero, leave it on the bench, or lose it to a ban. That
 * is deliberately a superset of what the API's own `draftedHeroIds` carries,
 * which excludes bans (`BANNED_HERO_DRAFTED` keeps picks and bans disjoint);
 * `toPayload` subtracts them back out at save time. Holding the superset here is
 * the whole point of the screen: it is the list the admin reads off the match
 * card, and it is what every hero dropdown below filters down to.
 */
interface SideForm {
  playerLabel: string
  draftedHeroIds: number[]
  /** Struck out of *this* side's draft. Every `heroId` here is also on `draftedHeroIds`. */
  bans: { heroId: number; banType: SidedBanType }[]
}

interface MatchForm {
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
   * this form cannot show a ban it cannot place, so it asks the admin to assign
   * one before saving rather than silently dropping or guessing it.
   */
  unassignedBans: { heroId: number; banType: SidedBanType }[]
}

function blankGame(gameNumber: number): MatchGameRequest {
  return {
    gameNumber,
    mapId: 0,
    participants: [
      { heroId: 0, healthRemaining: 0, isWinner: false },
      { heroId: 0, healthRemaining: 0, isWinner: false },
    ],
  }
}

function blankSide(): SideForm {
  return { playerLabel: '', draftedHeroIds: [], bans: [] }
}

function blankForm(): MatchForm {
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

const form = ref<MatchForm>(blankForm())

const tournaments = computed(() => tournamentsStore.tournaments)

// form.playedAt is always a full ISO instant (what the server sends and expects),
// but <input type="datetime-local"> only accepts/emits "YYYY-MM-DDTHH:mm" — anything
// else is silently sanitized to "" by the browser. This bridges the two formats
// instead of binding the input straight to form.playedAt.
const playedAtLocal = computed({
  get() {
    const iso = form.value.playedAt
    if (!iso) return ''
    const date = new Date(iso)
    if (Number.isNaN(date.getTime())) return ''
    const pad = (n: number) => String(n).padStart(2, '0')
    return (
      `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
      `T${pad(date.getHours())}:${pad(date.getMinutes())}`
    )
  },
  set(local: string) {
    if (!local) return
    const date = new Date(local)
    if (Number.isNaN(date.getTime())) return
    form.value.playedAt = date.toISOString()
  },
})

// Reset form based on mode
function resetForm() {
  form.value = props.prefill ? formFromPreview(props.prefill) : blankForm()
}

/**
 * Seeds the form from an import preview.
 *
 * The preview splits a side's arsenal the way the *API* does — `draftedHeroIds`
 * without the banned heroes, and the bans in their own list carrying the side
 * they came out of. This form holds the two together, so the seeding here is a
 * union rather than the subtraction the old partial-draft model needed.
 *
 * `round` is never in a preview — the source site names its rounds rather than
 * numbering them — so it keeps the blank form's default for the admin to set.
 * `playedAt` falls back the same way when the source's timezone was unreadable.
 */
function formFromPreview(preview: MatchImportPreviewDto): MatchForm {
  const blank = blankForm()

  return {
    round: blank.round,
    playedAt: preview.playedAt ?? blank.playedAt,
    externalLink: preview.sourceUrl,
    sides: [0, 1].map((side) => {
      const participant = preview.participants[side]
      // 0 is this form's "nothing selected" sentinel, so a ban the importer
      // could not resolve becomes an unfilled row rather than a broken option.
      const bans = preview.bans
        .filter((ban) => ban.banType !== 'PRE_BAN' && ban.side === side)
        .map((ban) => ({ heroId: ban.heroId ?? 0, banType: ban.banType as SidedBanType }))
      return {
        playerLabel: participant?.playerLabel ?? '',
        draftedHeroIds: [
          ...(participant?.draftedHeroIds ?? []),
          ...bans.map((ban) => ban.heroId).filter((heroId) => heroId !== 0),
        ],
        bans,
      }
    }),
    games: preview.games.map((game) => ({
      gameNumber: game.gameNumber,
      mapId: game.mapId ?? 0,
      participants: [0, 1].map((side) => ({
        heroId: game.participants[side]?.heroId ?? 0,
        healthRemaining: game.participants[side]?.healthRemaining ?? 0,
        isWinner: game.participants[side]?.isWinner ?? false,
      })),
    })),
    preBans: preview.bans
      .filter((ban) => ban.banType === 'PRE_BAN')
      .map((ban) => ({ heroId: ban.heroId ?? 0 })),
    // The importer always attributes a typed ban, since the source groups them
    // under the side that owned the hero — so this is empty in practice and
    // exists only so an unsided one could never vanish silently.
    unassignedBans: preview.bans
      .filter((ban) => ban.banType !== 'PRE_BAN' && ban.side === undefined)
      .map((ban) => ({ heroId: ban.heroId ?? 0, banType: ban.banType as SidedBanType })),
  }
}

/** Reads a saved match back into the form, the same union `formFromPreview` does. */
function formFromMatch(matchData: MatchResultDto): MatchForm {
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

// Load match data for edit mode
async function loadMatchData() {
  if (props.mode !== 'edit' || !props.tournamentId || !props.matchId) return

  loading.value = true
  serverError.value = null
  violations.value = []

  try {
    form.value = formFromMatch(await api.admin.getMatch(props.tournamentId, props.matchId))
  } catch (e) {
    serverError.value = describeError(e, 'Failed to load match data')
    violations.value = violationMessages(e)
  } finally {
    loading.value = false
  }
}

async function loadPools() {
  const tournamentId = props.tournamentId
  if (!tournamentId) return

  const [maps, heroes] = await Promise.all([
    api.admin.listMapPool(tournamentId),
    api.admin.listHeroPool(tournamentId),
  ])
  mapPool.value = maps.sort(byName)
  heroPool.value = heroes.sort(byName)
}

function addGame() {
  form.value.games.push(blankGame(form.value.games.length + 1))
}

function removeGame(index: number) {
  form.value.games.splice(index, 1)
  // Game numbers are always a dense 1..N sequence, so a removal renumbers the rest.
  form.value.games.forEach((game, i) => {
    game.gameNumber = i + 1
  })
}

function addPreBan() {
  form.value.preBans.push({ heroId: 0 })
}

function removePreBan(index: number) {
  form.value.preBans.splice(index, 1)
}

function addSideBan(side: number) {
  form.value.sides[side].bans.push({ heroId: 0, banType: 'OPPONENT_BAN' })
}

function removeSideBan(side: number, index: number) {
  form.value.sides[side].bans.splice(index, 1)
}

/** Moves an unsided ban onto the side whose draft it came out of. */
function assignBanToSide(index: number, side: number) {
  const [ban] = form.value.unassignedBans.splice(index, 1)
  form.value.sides[side].bans.push(ban)
  if (ban.heroId !== 0 && !form.value.sides[side].draftedHeroIds.includes(ban.heroId)) {
    form.value.sides[side].draftedHeroIds.push(ban.heroId)
  }
}

/** The heroes a side fielded, read off the form — a side is the participant's list position. */
function fieldedInForm(side: number): number[] {
  return form.value.games.flatMap((game) => {
    const participant = game.participants[side]
    return participant && participant.heroId !== 0 ? [participant.heroId] : []
  })
}

/** The heroes this side lost to a ban — off its arsenal, and so off every dropdown below. */
function bannedBySide(side: number): number[] {
  return form.value.sides[side]?.bans.map((ban) => ban.heroId) ?? []
}

const preBannedHeroIds = computed(() => form.value.preBans.map((ban) => ban.heroId))

/**
 * What a side can actually field: its arsenal, less anything struck out of it.
 * This is the option list for that side's per-game hero dropdowns, and the
 * reason `PLAYED_HERO_NOT_DRAFTED` cannot fire on a submission this form built.
 */
function fieldableHeroes(side: number): Hero[] {
  const drafted = form.value.sides[side]?.draftedHeroIds ?? []
  const struck = new Set([...bannedBySide(side), ...preBannedHeroIds.value])
  return heroPool.value.filter((hero) => drafted.includes(hero.id) && !struck.has(hero.id))
}

/** A side's own ban options: its arsenal, less the heroes it has already struck on another row. */
function bannableHeroes(side: number, currentHeroId: number): Hero[] {
  const drafted = form.value.sides[side]?.draftedHeroIds ?? []
  const alreadyStruck = bannedBySide(side).filter((heroId) => heroId !== currentHeroId)
  return heroPool.value.filter((hero) => drafted.includes(hero.id) && !alreadyStruck.includes(hero.id))
}

/**
 * A pre-ban precedes the draft, so it can only name a hero neither side took.
 * Offering the drafted ones would be offering a `BANNED_HERO_DRAFTED` 422.
 */
function preBannableHeroes(currentHeroId: number): Hero[] {
  const drafted = new Set(form.value.sides.flatMap((side) => side.draftedHeroIds))
  const alreadyStruck = preBannedHeroIds.value.filter((heroId) => heroId !== currentHeroId)
  return heroPool.value.filter((hero) => !drafted.has(hero.id) && !alreadyStruck.includes(hero.id))
}

/** Heroes not yet on this side's arsenal — everything is draftable until it is drafted. */
function draftableHeroes(side: number, currentHeroId: number): Hero[] {
  const alreadyDrafted = (form.value.sides[side]?.draftedHeroIds ?? []).filter(
    (heroId) => heroId !== currentHeroId,
  )
  return heroPool.value.filter((hero) => !alreadyDrafted.includes(hero.id))
}

function addDraftPick(side: number) {
  form.value.sides[side].draftedHeroIds.push(0)
}

/**
 * Dropping a hero off a side's arsenal drops it everywhere that arsenal fed.
 *
 * Without the cascade a game select would keep a value no longer in its own
 * option list — the browser renders that as blank while the model still holds
 * the id, so the form would look filled and submit a hero the side never
 * drafted.
 */
function removeDraftPick(side: number, index: number) {
  const [heroId] = form.value.sides[side].draftedHeroIds.splice(index, 1)
  if (heroId === 0) return

  form.value.sides[side].bans = form.value.sides[side].bans.filter((ban) => ban.heroId !== heroId)
  form.value.games.forEach((game) => {
    const participant = game.participants[side]
    if (participant?.heroId === heroId) participant.heroId = 0
  })
}

function heroName(heroId: number): string {
  return heroPool.value.find((hero) => hero.id === heroId)?.name ?? `#${heroId}`
}

// Picking a side makes it the sole winner of that game and clears the other.
// There is no way to pick *neither*, and that is deliberate: every game is
// played to a decision, so the server rejects a game with no winner
// (NOT_EXACTLY_ONE_WINNER). The losing side must finish with 0 or less health;
// the winning side may have any health, including a negative value.
function setWinner(gameIndex: number, participantIndex: number) {
  form.value.games[gameIndex].participants.forEach((p, i) => {
    p.isWinner = i === participantIndex
  })
}

/**
 * The form's shape, translated to the API's.
 *
 * The one real conversion is the draft: this form holds a side's whole arsenal,
 * the API wants it without the banned heroes, since `BANNED_HERO_DRAFTED` keeps
 * picks and bans disjoint. Subtracting here rather than making the admin keep
 * two lists in their head is the point of the screen.
 */
function toPayload(): RecordMatchRequest {
  return {
    round: form.value.round,
    playedAt: form.value.playedAt,
    externalLink: form.value.externalLink.trim(),
    participants: form.value.sides.map((side) => ({
      playerLabel: side.playerLabel,
      draftedHeroIds: side.draftedHeroIds.filter(
        (heroId) => !side.bans.some((ban) => ban.heroId === heroId),
      ),
    })),
    games: form.value.games,
    bans: [
      ...form.value.sides.flatMap((sideForm, side) =>
        sideForm.bans.map((ban) => ({ heroId: ban.heroId, banType: ban.banType, side })),
      ),
      // A pre-ban is struck before sides exist, so it carries none — the server
      // rejects one that does (`BAN_SIDE_INVALID`).
      ...form.value.preBans.map((ban) => ({ heroId: ban.heroId, banType: 'PRE_BAN' as const, side: null })),
    ],
  }
}

/**
 * Everything this form can rule out on its own, as one message or null.
 *
 * It is a computed rather than a sequence of checks inside `saveMatch` on
 * purpose: an admin who fixes the problem in place sees the banner clear as
 * they type, instead of a stale complaint that survives until the next save.
 *
 * Most of what the old partial-draft form had to police is now unrepresentable:
 * a game dropdown only offers heroes that side drafted and did not lose to a
 * ban, so `PLAYED_HERO_NOT_DRAFTED` and `BANNED_HERO_PLAYED` cannot be built
 * here at all. What remains is either a half-filled row or a conflict the
 * dropdowns cannot see, because it spans two of them.
 */
const validationError = computed<string | null>(() => {
  const { games, sides, preBans, unassignedBans } = form.value

  // Required server-side, and the duplicate check depends on it. Caught here so
  // an untouched box reads as a prompt rather than a 400.
  if (form.value.externalLink.trim().length === 0) {
    return 'This match needs an external link — it is what stops the same match being recorded twice'
  }

  if (unassignedBans.length > 0) {
    return `Assign ${heroName(unassignedBans[0].heroId)}'s ban to the side it was struck from — this match predates per-side bans`
  }

  for (const [side, sideForm] of sides.entries()) {
    if (sideForm.draftedHeroIds.some((heroId) => heroId === 0)) {
      return `Every hero on side ${side + 1}'s draft needs to be chosen`
    }
    // DUPLICATE_PICK server-side; caught here so it reads as a prompt, not a 422.
    const seen = new Set<number>()
    for (const heroId of sideForm.draftedHeroIds) {
      if (seen.has(heroId)) {
        return `Side ${side + 1} drafted ${heroName(heroId)} twice — remove the duplicate row`
      }
      seen.add(heroId)
    }
    if (sideForm.bans.some((ban) => ban.heroId === 0)) return `Every ban on side ${side + 1} needs a hero selected`
  }

  if (preBans.some((ban) => ban.heroId === 0)) return 'Every pre-ban needs a hero selected'

  // DUPLICATE_BAN server-side: `hero_ban` is keyed (match_id, hero_id), so one
  // hero is struck at most once per series however many sides wanted it. Spans
  // three lists, so no single dropdown can prevent it.
  const allBans = [...sides.flatMap((side) => side.bans.map((ban) => ban.heroId)), ...preBans.map((ban) => ban.heroId)]
  const bannedTwice = allBans.find((heroId, index) => heroId !== 0 && allBans.indexOf(heroId) !== index)
  if (bannedTwice !== undefined) {
    return `${heroName(bannedTwice)} is banned twice — a hero can only be struck once per series`
  }

  // BANNED_HERO_DRAFTED server-side. A side's own bans come off its draft at
  // save time, but a pre-ban does not, so it must name a hero neither side took.
  const drafted = new Set(sides.flatMap((side) => side.draftedHeroIds))
  const preBannedButDrafted = preBans.find((ban) => ban.heroId !== 0 && drafted.has(ban.heroId))
  if (preBannedButDrafted) {
    return `${heroName(preBannedButDrafted.heroId)} is pre-banned, so neither side can have drafted it — a pre-ban precedes the draft`
  }

  if (games.length === 0) return 'At least one game is required'
  if (games.some((g) => g.mapId === 0)) return 'Every game needs a map selected'
  // The player name is deliberately not required: it is a free-text label, and an
  // unattributed result is still a valid result.
  if (games.some((g) => g.participants.some((p) => p.heroId === 0))) {
    return 'Every game needs a hero selected for both sides'
  }
  // A backstop, not a prompt the admin should ever see: the dropdowns cannot
  // offer an undrafted hero. It fires only if a hero left a draft while a game
  // still named it and the cascade in `removeDraftPick` somehow missed it.
  for (const [side] of sides.entries()) {
    const fieldable = fieldableHeroes(side).map((hero) => hero.id)
    const stray = fieldedInForm(side).find((heroId) => !fieldable.includes(heroId))
    if (stray !== undefined) {
      return `Side ${side + 1} fields ${heroName(stray)}, which is not on its draft — add it or pick another hero`
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
})

/** The server's complaint if there is one, otherwise this form's own. */
const error = computed(() => serverError.value ?? (submitAttempted.value ? validationError.value : null))

// A rejected save describes a payload that no longer exists the moment the
// admin edits the form, so the banner clears on the correction rather than
// standing until the next attempt. `validationError` needs no equivalent: it is
// computed off the form and re-evaluates itself.
watch(
  form,
  () => {
    if (!isInitialized.value) return
    serverError.value = null
    violations.value = []
  },
  { deep: true },
)

async function saveMatch() {
  submitAttempted.value = true
  serverError.value = null
  violations.value = []

  const tournamentId = props.tournamentId
  if (!tournamentId) {
    serverError.value = 'Tournament ID is required'
    return
  }
  if (validationError.value) return

  loading.value = true

  try {
    if (props.mode === 'create') {
      await api.admin.recordMatch(tournamentId, toPayload())
    } else if (props.mode === 'edit' && props.matchId) {
      await api.admin.correctMatch(tournamentId, props.matchId, toPayload())
    }
    emit('success')
  } catch (e) {
    serverError.value = describeError(e, 'Failed to save match')
    violations.value = violationMessages(e)
  } finally {
    loading.value = false
  }
}

function handleCancel() {
  emit('cancel')
}

// Initialize based on mode
onMounted(async () => {
  loading.value = true
  try {
    await loadPools()
    if (props.mode === 'edit') {
      await loadMatchData()
    } else {
      resetForm()
    }
  } catch (e) {
    serverError.value = describeError(e, 'Failed to load map and hero pools')
    violations.value = violationMessages(e)
  } finally {
    loading.value = false
  }
  isInitialized.value = true
})
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">
        {{ mode === 'create' ? 'Record Match Result' : 'Edit Match Result' }}
      </h2>
    </div>

    <ErrorBanner :message="error" :violations="violations" />

    <!-- Tournament info (when provided as prop) -->
    <div v-if="tournamentId" class="panel flex items-center gap-4 p-4">
      <div class="label-caps">Tournament</div>
      <div class="headline text-base text-cyan">
        {{ tournaments.find(t => t.id === tournamentId)?.name || `ID: ${tournamentId}` }}
      </div>
    </div>

    <!-- Loading state -->
    <div v-if="loading && !isInitialized" class="p-12 text-center font-mono text-ink-dim">
      Loading...
    </div>

    <!-- Form -->
    <div v-else class="panel flex flex-col gap-6 p-6">
      <h3 class="headline text-lg text-cyan">{{ mode === 'create' ? 'Record New Match' : 'Edit Match' }}</h3>

      <div class="grid gap-4 sm:grid-cols-2">
        <div class="flex flex-col gap-2">
          <label for="match-round" class="label-caps">Round *</label>
          <input
            id="match-round"
            v-model.number="form.round"
            type="number"
            min="1"
            class="field-input"
          />
        </div>

        <div class="flex flex-col gap-2">
          <label for="match-played-at" class="label-caps">Played At (ISO timestamp) *</label>
          <input
            id="match-played-at"
            v-model="playedAtLocal"
            type="datetime-local"
            class="field-input"
          />
        </div>

        <div class="flex flex-col gap-2 sm:col-span-2">
          <label for="match-external-link" class="label-caps">External Link</label>
          <input
            id="match-external-link"
            v-model="form.externalLink"
            type="text"
            placeholder="https://…"
            class="field-input"
          />
          <p class="text-xs text-ink-dim">
            Where this match is recorded elsewhere — the import source, a bracket page, a VOD. It
            has to be unique within the tournament: it is what stops the same match being recorded
            twice. For a match with no page anywhere, any identifier of your own will do.
          </p>
        </div>
      </div>

      <!-- Bans that predate per-side attribution. Nothing else can be filled in
           sensibly until these are placed, so they sit above the drafts. -->
      <div
        v-if="form.unassignedBans.length"
        class="flex flex-col gap-3 border border-magenta bg-surface-lowest p-4"
      >
        <h4 class="headline text-base text-magenta">Bans with no side</h4>
        <p class="font-mono text-xs leading-relaxed text-ink-dim">
          This match was recorded before a ban stored which draft it came out of. Place each one on
          the side whose hero it was — the type already says who struck it.
        </p>
        <div
          v-for="(ban, index) in form.unassignedBans"
          :key="index"
          class="flex flex-col gap-2 border border-edge bg-surface-low p-3 sm:flex-row sm:items-center sm:justify-between"
        >
          <span class="font-mono text-sm">
            {{ heroName(ban.heroId) }}
            <span class="text-xs text-ink-dim">({{ banTypeLabels[ban.banType] }})</span>
          </span>
          <div class="flex gap-2">
            <button
              v-for="side in [0, 1]"
              :key="side"
              type="button"
              class="btn-ghost px-4 py-2 text-xs"
              @click="assignBanToSide(index, side)"
            >
              Side {{ side + 1 }}
            </button>
          </div>
        </div>
      </div>

      <!-- The draft comes first, and everything below it is filtered to what it
           says. A side's list is its whole arsenal: the heroes it fielded, the
           ones it benched, and the ones it lost to a ban. -->
      <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
        <h4 class="headline text-base text-cyan">Drafts (exactly 2 sides, for the whole series)</h4>
        <p class="font-mono text-xs leading-relaxed text-ink-dim">
          Enter every hero each side took, whether or not it reached the table. The games and bans
          below only offer that side's own heroes.
        </p>

        <div
          v-for="(side, index) in form.sides"
          :key="index"
          class="flex flex-col gap-4 border border-edge bg-surface-low p-4"
        >
          <div class="flex flex-col gap-4 sm:flex-row sm:items-center">
            <div class="flex size-8 shrink-0 items-center justify-center bg-cyan font-display text-xl text-surface-lowest">
              {{ index + 1 }}
            </div>
            <div class="flex flex-1 flex-col gap-2">
              <label :for="`player-${index}`" class="label-caps">Player name (optional)</label>
              <input
                :id="`player-${index}`"
                v-model="side.playerLabel"
                type="text"
                class="field-input-sm"
                placeholder="Who piloted this side"
              />
            </div>
          </div>

          <div class="flex flex-col gap-3 border-t border-edge pt-3">
            <span class="label-caps">Drafted heroes</span>

            <div
              v-for="(_heroId, pickIndex) in side.draftedHeroIds"
              :key="pickIndex"
              class="flex flex-col gap-2 sm:flex-row sm:items-end"
            >
              <div class="flex flex-1 flex-col gap-2">
                <label :for="`draft-${index}-${pickIndex}`" class="label-caps">Hero {{ pickIndex + 1 }}</label>
                <select
                  :id="`draft-${index}-${pickIndex}`"
                  v-model.number="side.draftedHeroIds[pickIndex]"
                  class="field-input-sm"
                >
                  <option :value="0" disabled>Select a hero…</option>
                  <option
                    v-for="hero in draftableHeroes(index, side.draftedHeroIds[pickIndex])"
                    :key="hero.id"
                    :value="hero.id"
                  >
                    {{ hero.name }}
                  </option>
                </select>
              </div>
              <button type="button" class="btn-ghost px-4 py-2 text-xs" @click="removeDraftPick(index, pickIndex)">
                Remove
              </button>
            </div>

            <button type="button" class="btn-ghost" @click="addDraftPick(index)">+ Add Drafted Hero</button>
          </div>

          <!-- This side's bans: heroes struck out of the arsenal above. They
               score a ban instead of an appearance, which is why they belong on
               the draft rather than beside it. -->
          <div class="flex flex-col gap-3 border-t border-edge pt-3">
            <span class="label-caps">Struck out of this draft (optional)</span>

            <div
              v-for="(ban, banIndex) in side.bans"
              :key="banIndex"
              class="flex flex-col gap-4 sm:flex-row sm:items-end"
            >
              <div class="grid flex-1 grid-cols-2 gap-4">
                <div class="flex flex-col gap-2">
                  <label :for="`ban-hero-${index}-${banIndex}`" class="label-caps">Hero</label>
                  <select
                    :id="`ban-hero-${index}-${banIndex}`"
                    v-model.number="ban.heroId"
                    class="field-input-sm"
                  >
                    <option :value="0" disabled>
                      {{ side.draftedHeroIds.length ? 'Select a hero…' : `Draft heroes for side ${index + 1} first` }}
                    </option>
                    <option v-for="hero in bannableHeroes(index, ban.heroId)" :key="hero.id" :value="hero.id">
                      {{ hero.name }}
                    </option>
                  </select>
                </div>

                <div class="flex flex-col gap-2">
                  <label :for="`ban-type-${index}-${banIndex}`" class="label-caps">Struck by</label>
                  <select
                    :id="`ban-type-${index}-${banIndex}`"
                    v-model="ban.banType"
                    class="field-input-sm"
                  >
                    <option v-for="type in sidedBanTypes" :key="type.value" :value="type.value">{{ type.label }}</option>
                  </select>
                </div>
              </div>

              <button type="button" class="btn-ghost px-4 py-2 text-xs" @click="removeSideBan(index, banIndex)">
                Remove
              </button>
            </div>

            <button type="button" class="btn-ghost" @click="addSideBan(index)">+ Add Ban</button>
          </div>
        </div>
      </div>

      <!-- Games -->
      <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
        <h4 class="headline text-base text-cyan">Games (best-of-N — at least 1 required)</h4>

        <div
          v-for="(game, gameIndex) in form.games"
          :key="gameIndex"
          class="flex flex-col gap-4 border border-edge bg-surface-low p-4"
        >
          <div class="flex items-center justify-between">
            <h5 class="label-caps text-cyan">Game {{ game.gameNumber }}</h5>
            <button
              v-if="form.games.length > 1"
              type="button"
              class="btn-ghost px-4 py-2 text-xs"
              @click="removeGame(gameIndex)"
            >
              Remove Game
            </button>
          </div>

          <div class="flex flex-col gap-2">
            <label :for="`game-${gameIndex}-map`" class="label-caps">Map</label>
            <select
              :id="`game-${gameIndex}-map`"
              v-model.number="game.mapId"
              class="field-input-sm"
            >
              <option :value="0" disabled>Select a map…</option>
              <option v-for="map in mapPool" :key="map.id" :value="map.id">{{ map.name }}</option>
            </select>
          </div>

          <div
            v-for="(participant, participantIndex) in game.participants"
            :key="participantIndex"
            class="grid grid-cols-2 gap-4 border border-edge bg-surface-lowest p-3 sm:grid-cols-4"
          >
            <div class="flex flex-col gap-2">
              <label :for="`game-${gameIndex}-hero-${participantIndex}`" class="label-caps">
                Side {{ participantIndex + 1 }} Hero
              </label>
              <!-- Only what this side drafted and did not lose to a ban, which is
                   what makes PLAYED_HERO_NOT_DRAFTED unreachable from this form. -->
              <select
                :id="`game-${gameIndex}-hero-${participantIndex}`"
                v-model.number="participant.heroId"
                class="field-input-sm"
              >
                <option :value="0" disabled>
                  {{
                    fieldableHeroes(participantIndex).length
                      ? 'Select a hero…'
                      : `Draft heroes for side ${participantIndex + 1} first`
                  }}
                </option>
                <option v-for="hero in fieldableHeroes(participantIndex)" :key="hero.id" :value="hero.id">
                  {{ hero.name }}
                </option>
              </select>
            </div>

            <div class="flex flex-col gap-2">
              <label :for="`game-${gameIndex}-health-${participantIndex}`" class="label-caps">Health (loser: 0 or less)</label>
              <input
                :id="`game-${gameIndex}-health-${participantIndex}`"
                v-model.number="participant.healthRemaining"
                type="number"
                class="field-input-sm"
              />
            </div>

            <div class="flex flex-col gap-2">
              <!-- A radio, not a checkbox: exactly one side wins each game, and
                   there is no "neither" to express. -->
              <label class="flex cursor-pointer items-center gap-2 pt-5 font-mono text-xs text-ink">
                <input
                  type="radio"
                  :name="`game-${gameIndex}-winner`"
                  :checked="participant.isWinner"
                  @change="setWinner(gameIndex, participantIndex)"
                />
                <span>Winner</span>
              </label>
            </div>
          </div>
        </div>

        <button type="button" class="btn-ghost" @click="addGame">+ Add Game</button>
      </div>

      <!-- Pre-bans: struck before sides were assigned, so they belong to neither
           draft and score neither ban metric. -->
      <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
        <h4 class="headline text-base text-cyan">Pre-bans (optional)</h4>
        <p class="font-mono text-xs leading-relaxed text-ink-dim">
          Heroes struck before sides were known. They belong to neither draft, so only heroes nobody
          drafted are offered.
        </p>

        <div
          v-for="(ban, index) in form.preBans"
          :key="index"
          class="flex flex-col gap-4 border border-edge bg-surface-low p-4 sm:flex-row sm:items-end"
        >
          <div class="flex flex-1 flex-col gap-2">
            <label :for="`pre-ban-hero-${index}`" class="label-caps">Hero</label>
            <select
              :id="`pre-ban-hero-${index}`"
              v-model.number="ban.heroId"
              class="field-input-sm"
            >
              <option :value="0" disabled>Select a hero…</option>
              <option v-for="hero in preBannableHeroes(ban.heroId)" :key="hero.id" :value="hero.id">
                {{ hero.name }}
              </option>
            </select>
          </div>

          <button type="button" class="btn-ghost px-4 py-2 text-xs" @click="removePreBan(index)">
            Remove
          </button>
        </div>

        <button type="button" class="btn-ghost" @click="addPreBan">+ Add Pre-ban</button>
      </div>

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="handleCancel">Cancel</button>
        <button class="btn-primary" :disabled="loading" @click="saveMatch">
          {{ loading ? (mode === 'create' ? 'Recording...' : 'Updating...') : (mode === 'create' ? 'Record Match' : 'Update Match') }}
        </button>
      </div>

      <div v-if="!loading && (mapPool.length === 0 || heroPool.length === 0)" class="border border-edge bg-surface-lowest p-4">
        <p class="mb-2 font-mono text-xs text-cyan uppercase">Nothing to pick from yet</p>
        <p class="font-mono text-xs leading-relaxed text-ink-dim">
          <span v-if="mapPool.length === 0">This tournament's map pool is empty — add one in the Maps section. </span>
          <span v-if="heroPool.length === 0">This tournament's hero pool is empty — price a hero in first. </span>
          Player names are free text: type anything, or leave it blank. Points are scored per hero,
          so the name is only ever shown, never counted.
        </p>
      </div>
    </div>
  </div>
</template>
