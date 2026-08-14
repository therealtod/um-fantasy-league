<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { api, describeError, violationMessages } from '@/api/client'
import { useTournamentsStore } from '@/stores/tournaments'
import { byName } from '@/lib/sort'
import type { BanType, Hero, MapAdminDto, RecordMatchRequest } from '@/api/types'
import ErrorBanner from '@/components/ErrorBanner.vue'

interface Props {
  tournamentId?: number
  matchId?: number
  mode: 'create' | 'edit'
}

const props = defineProps<Props>()
const emit = defineEmits<{
  success: []
  cancel: []
}>()

const tournamentsStore = useTournamentsStore()

const loading = ref(false)
const error = ref<string | null>(null)
const violations = ref<string[]>([])
const isInitialized = ref(false)

const banTypes: { value: BanType; label: string }[] = [
  { value: 'PRE_BAN', label: 'Pre-ban' },
  { value: 'OPPONENT_BAN', label: 'Opponent ban' },
  { value: 'SELF_BAN', label: 'Self ban' },
]

// The tournament's legal boards and priced hero pool — what the match actually
// scores against — so the admin picks from a list instead of typing a raw id.
const mapPool = ref<MapAdminDto[]>([])
const heroPool = ref<Hero[]>([])

function blankGame(gameNumber: number) {
  return {
    gameNumber,
    mapId: 0,
    participants: [
      { heroId: 0, healthRemaining: 0, isWinner: false },
      { heroId: 0, healthRemaining: 0, isWinner: false },
    ],
  }
}

function blankForm(): RecordMatchRequest {
  return {
    round: 1,
    playedAt: new Date().toISOString(),
    externalLink: '',
    participants: [{ playerLabel: '' }, { playerLabel: '' }],
    games: [blankGame(1)],
    bans: [],
  }
}

const form = ref<RecordMatchRequest>(blankForm())

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
  form.value = blankForm()
}

// Load match data for edit mode
async function loadMatchData() {
  if (props.mode !== 'edit' || !props.tournamentId || !props.matchId) return

  loading.value = true
  error.value = null
  violations.value = []

  try {
    const matchData = await api.admin.getMatch(props.tournamentId, props.matchId)

    // Convert MatchResultDto to RecordMatchRequest
    form.value = {
      round: matchData.round,
      playedAt: matchData.playedAt,
      externalLink: matchData.externalLink ?? '',
      participants: [...matchData.participants]
        .sort((a, b) => a.side - b.side)
        .map((p) => ({ playerLabel: p.playerLabel ?? '' })),
      games: [...matchData.games]
        .sort((a, b) => a.gameNumber - b.gameNumber)
        .map((g) => ({
          gameNumber: g.gameNumber,
          mapId: g.mapId,
          participants: [...g.participants]
            .sort((a, b) => a.side - b.side)
            .map((p) => ({ heroId: p.heroId, healthRemaining: p.healthRemaining, isWinner: p.isWinner })),
        })),
      bans: matchData.bans.map((b) => ({ heroId: b.heroId, banType: b.banType })),
    }
  } catch (e) {
    error.value = describeError(e, 'Failed to load match data')
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

function addBan() {
  form.value.bans.push({ heroId: 0, banType: 'PRE_BAN' })
}

function removeBan(index: number) {
  form.value.bans.splice(index, 1)
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

async function saveMatch() {
  violations.value = []

  const tournamentId = props.tournamentId
  if (!tournamentId) {
    error.value = 'Tournament ID is required'
    return
  }
  if (form.value.games.length === 0) {
    error.value = 'At least one game is required'
    return
  }
  if (form.value.games.some((g) => g.mapId === 0)) {
    error.value = 'Every game needs a map selected'
    return
  }
  // The player name is deliberately not required: it is a free-text label, and an
  // unattributed result is still a valid result.
  if (form.value.games.some((g) => g.participants.some((p) => p.heroId === 0))) {
    error.value = 'Every game needs a hero selected for both sides'
    return
  }
  // The server rejects this too (NOT_EXACTLY_ONE_WINNER) — caught here so an
  // untouched winner radio reads as a prompt rather than a 422.
  if (form.value.games.some((g) => g.participants.filter((p) => p.isWinner).length !== 1)) {
    error.value = 'Every game needs exactly one winner — a game cannot end in a draw'
    return
  }
  if (form.value.games.some((g) => g.participants.some((p) => !p.isWinner && p.healthRemaining > 0))) {
    error.value = 'The losing hero must have 0 or less health'
    return
  }
  if (form.value.bans.some((b) => b.heroId === 0)) {
    error.value = 'Every ban needs a hero selected'
    return
  }

  loading.value = true
  error.value = null
  violations.value = []

  try {
    if (props.mode === 'create') {
      await api.admin.recordMatch(tournamentId, form.value)
    } else if (props.mode === 'edit' && props.matchId) {
      await api.admin.correctMatch(tournamentId, props.matchId, form.value)
    }
    emit('success')
  } catch (e) {
    error.value = describeError(e, 'Failed to save match')
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
    error.value = describeError(e, 'Failed to load map and hero pools')
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
          <label for="match-external-link" class="label-caps">External Link (optional)</label>
          <input
            id="match-external-link"
            v-model="form.externalLink"
            type="text"
            placeholder="https://…"
            class="field-input"
          />
        </div>
      </div>

      <!-- Participants -->
      <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
        <h4 class="headline text-base text-cyan">Participants (exactly 2 required, for the whole series)</h4>

        <div
          v-for="(participant, index) in form.participants"
          :key="index"
          class="flex flex-col gap-4 border border-edge bg-surface-low p-4 sm:flex-row sm:items-center"
        >
          <div class="flex size-8 shrink-0 items-center justify-center bg-cyan font-display text-xl text-surface-lowest">
            {{ index + 1 }}
          </div>
          <div class="flex flex-1 flex-col gap-2">
            <label :for="`player-${index}`" class="label-caps">Player name (optional)</label>
            <input
              :id="`player-${index}`"
              v-model="participant.playerLabel"
              type="text"
              class="field-input-sm"
              placeholder="Who piloted this side"
            />
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
              <select
                :id="`game-${gameIndex}-hero-${participantIndex}`"
                v-model.number="participant.heroId"
                class="field-input-sm"
              >
                <option :value="0" disabled>Select a hero…</option>
                <option v-for="hero in heroPool" :key="hero.id" :value="hero.id">{{ hero.name }}</option>
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

      <!-- Bans -->
      <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
        <h4 class="headline text-base text-cyan">Bans (optional, once for the whole series)</h4>

        <div
          v-for="(ban, index) in form.bans"
          :key="index"
          class="flex flex-col gap-4 border border-edge bg-surface-low p-4 sm:flex-row sm:items-start"
        >
          <div class="grid flex-1 grid-cols-2 gap-4">
            <div class="flex flex-col gap-2">
              <label :for="`ban-hero-${index}`" class="label-caps">Hero</label>
              <select
                :id="`ban-hero-${index}`"
                v-model.number="ban.heroId"
                class="field-input-sm"
              >
                <option :value="0" disabled>Select a hero…</option>
                <option v-for="hero in heroPool" :key="hero.id" :value="hero.id">{{ hero.name }}</option>
              </select>
            </div>

            <div class="flex flex-col gap-2">
              <label :for="`ban-type-${index}`" class="label-caps">Type</label>
              <select
                :id="`ban-type-${index}`"
                v-model="ban.banType"
                class="field-input-sm"
              >
                <option v-for="type in banTypes" :key="type.value" :value="type.value">{{ type.label }}</option>
              </select>
            </div>
          </div>

          <button type="button" class="btn-ghost px-4 py-2 text-xs" @click="removeBan(index)">
            Remove
          </button>
        </div>

        <button type="button" class="btn-ghost" @click="addBan">+ Add Ban</button>
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
