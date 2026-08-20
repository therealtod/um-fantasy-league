<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { api, describeError, violationMessages } from '@/api/client'
import { useTournamentsStore } from '@/stores/tournaments'
import { byName } from '@/lib/sort'
import type { BanType, Hero, MapAdminDto, MatchImportPreviewDto } from '@/api/types'
// The form model, the seeding, the payload conversion, the option lists and the
// validation all live in the domain module, as plain functions over plain data.
// What is left here is the reactive wrapper, the template, and the API calls —
// so a `matchForm.` call site is the marker for "this is a rule, tested in
// matchForm.spec.ts", and anything without one is rendering.
import * as matchForm from '@/domain/matchForm'
import type { MatchForm, SidedBanType } from '@/domain/matchForm'
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

const form = ref<MatchForm>(matchForm.blankForm())

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

function resetForm() {
  form.value = props.prefill ? matchForm.formFromPreview(props.prefill) : matchForm.blankForm()
}

async function loadMatchData() {
  if (props.mode !== 'edit' || !props.tournamentId || !props.matchId) return

  loading.value = true
  serverError.value = null
  violations.value = []

  try {
    form.value = matchForm.formFromMatch(await api.admin.getMatch(props.tournamentId, props.matchId))
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

/* The plain add/remove handlers. Anything with a consequence beyond the row it
   touches — a renumbering, a cascade, a winner moving sides — is in the domain
   module instead. */

function addGame() {
  form.value.games.push(matchForm.blankGame(form.value.games.length + 1))
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

function addDraftPick(side: number) {
  form.value.sides[side].draftedHeroIds.push(0)
}

/* Template-facing adapters: the domain functions take the form and the pool,
   which the template has no reason to repeat at every call site. */

const removeGame = (index: number) => matchForm.removeGame(form.value, index)
const removeDraftPick = (side: number, index: number) => matchForm.removeDraftPick(form.value, side, index)
const assignBanToSide = (index: number, side: number) => matchForm.assignBanToSide(form.value, index, side)
const setWinner = (gameIndex: number, participantIndex: number) =>
  matchForm.setWinner(form.value, gameIndex, participantIndex)

const heroName = (heroId: number) => matchForm.heroName(heroPool.value, heroId)
const fieldableHeroes = (side: number) => matchForm.fieldableHeroes(form.value, heroPool.value, side)
const bannableHeroes = (side: number, currentHeroId: number) =>
  matchForm.bannableHeroes(form.value, heroPool.value, side, currentHeroId)
const preBannableHeroes = (currentHeroId: number) =>
  matchForm.preBannableHeroes(form.value, heroPool.value, currentHeroId)
const draftableHeroes = (side: number, currentHeroId: number) =>
  matchForm.draftableHeroes(form.value, heroPool.value, side, currentHeroId)

/**
 * What this form can rule out on its own, live.
 *
 * A computed rather than a step inside `saveMatch` on purpose: an admin who
 * fixes the problem in place sees the banner clear as they type, instead of a
 * stale complaint that survives until the next save.
 */
const validationError = computed(() => matchForm.validate(form.value, heroPool.value))

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
      await api.admin.recordMatch(tournamentId, matchForm.toPayload(form.value))
    } else if (props.mode === 'edit' && props.matchId) {
      await api.admin.correctMatch(tournamentId, props.matchId, matchForm.toPayload(form.value))
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
