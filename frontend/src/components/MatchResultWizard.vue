<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { api } from '@/api/client'
import { useTournamentsStore } from '@/stores/tournaments'
import type { Hero, MapAdminDto, RecordMatchRequest } from '@/api/types'

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
const isInitialized = ref(false)

// The tournament's legal boards and priced hero pool — what the match actually
// scores against — so the admin picks from a list instead of typing a raw id.
const mapPool = ref<MapAdminDto[]>([])
const heroPool = ref<Hero[]>([])

function blankForm(): RecordMatchRequest {
  return {
    round: 1,
    mapId: 0,
    playedAt: new Date().toISOString(),
    participants: [
      { playerLabel: '', heroId: 0, healthRemaining: 0, isWinner: false },
      { playerLabel: '', heroId: 0, healthRemaining: 0, isWinner: false },
    ],
    bans: [],
  }
}

const form = ref<RecordMatchRequest>(blankForm())

const tournaments = computed(() => tournamentsStore.tournaments)

// Reset form based on mode
function resetForm() {
  form.value = blankForm()
}

// Load match data for edit mode
async function loadMatchData() {
  if (props.mode !== 'edit' || !props.tournamentId || !props.matchId) return

  loading.value = true
  error.value = null

  try {
    const matchData = await api.admin.getMatch(props.tournamentId, props.matchId)

    // Convert MatchResultDto to RecordMatchRequest
    form.value = {
      round: matchData.round,
      mapId: matchData.mapId,
      playedAt: matchData.playedAt,
      participants: matchData.participants.map(p => ({
        playerLabel: p.playerLabel ?? '',
        heroId: p.heroId,
        healthRemaining: p.healthRemaining,
        isWinner: p.isWinner
      })),
      bans: matchData.bans.map(b => ({ heroId: b.heroId }))
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load match data'
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
  mapPool.value = maps
  heroPool.value = heroes
}

function addBan() {
  form.value.bans.push({ heroId: 0 })
}

function removeBan(index: number) {
  form.value.bans.splice(index, 1)
}

function setWinner(participantIndex: number) {
  form.value.participants.forEach((p, i) => {
    p.isWinner = i === participantIndex
  })
}

async function saveMatch() {
  const tournamentId = props.tournamentId
  if (!tournamentId) {
    error.value = 'Tournament ID is required'
    return
  }
  if (form.value.mapId === 0) {
    error.value = 'A map is required'
    return
  }
  // The player name is deliberately not required: it is a free-text label, and an
  // unattributed result is still a valid result.
  if (form.value.participants.some((p) => p.heroId === 0)) {
    error.value = 'All participants must have a hero selected'
    return
  }

  loading.value = true
  error.value = null

  try {
    if (props.mode === 'create') {
      await api.admin.recordMatch(tournamentId, form.value)
    } else if (props.mode === 'edit' && props.matchId) {
      await api.admin.correctMatch(tournamentId, props.matchId, form.value)
    }
    emit('success')
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to save match'
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
    error.value = e instanceof Error ? e.message : 'Failed to load map and hero pools'
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

    <p v-if="error" class="border border-magenta/50 bg-magenta/10 p-4 font-mono text-sm text-magenta">
      {{ error }}
    </p>

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
            class="border border-edge bg-surface-lowest px-3 py-2 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
          />
        </div>

        <div class="flex flex-col gap-2">
          <label for="match-map" class="label-caps">Map *</label>
          <select
            id="match-map"
            v-model.number="form.mapId"
            class="border border-edge bg-surface-lowest px-3 py-2 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
          >
            <option :value="0" disabled>Select a map…</option>
            <option v-for="map in mapPool" :key="map.id" :value="map.id">{{ map.name }}</option>
          </select>
        </div>

        <div class="flex flex-col gap-2 sm:col-span-2">
          <label for="match-played-at" class="label-caps">Played At (ISO timestamp) *</label>
          <input
            id="match-played-at"
            v-model="form.playedAt"
            type="datetime-local"
            class="border border-edge bg-surface-lowest px-3 py-2 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
          />
        </div>
      </div>

      <!-- Participants -->
      <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
        <h4 class="headline text-base text-cyan">Participants (exactly 2 required)</h4>

        <div
          v-for="(participant, index) in form.participants"
          :key="index"
          class="flex flex-col gap-4 border border-edge bg-surface-low p-4 sm:flex-row sm:items-start"
        >
          <div class="flex size-8 shrink-0 items-center justify-center bg-cyan font-display text-xl text-surface-lowest">
            {{ index + 1 }}
          </div>
          <div class="grid flex-1 grid-cols-2 gap-4 sm:grid-cols-4">
            <div class="flex flex-col gap-2">
              <label :for="`player-${index}`" class="label-caps">Player name (optional)</label>
              <input
                :id="`player-${index}`"
                v-model="participant.playerLabel"
                type="text"
                class="border border-edge bg-surface-lowest px-2 py-1.5 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
                placeholder="Who piloted this hero"
              />
            </div>

            <div class="flex flex-col gap-2">
              <label :for="`hero-${index}`" class="label-caps">Hero</label>
              <select
                :id="`hero-${index}`"
                v-model.number="participant.heroId"
                class="border border-edge bg-surface-lowest px-2 py-1.5 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
              >
                <option :value="0" disabled>Select a hero…</option>
                <option v-for="hero in heroPool" :key="hero.id" :value="hero.id">{{ hero.name }}</option>
              </select>
            </div>

            <div class="flex flex-col gap-2">
              <label :for="`health-${index}`" class="label-caps">Health Remaining</label>
              <input
                :id="`health-${index}`"
                v-model.number="participant.healthRemaining"
                type="number"
                min="0"
                class="border border-edge bg-surface-lowest px-2 py-1.5 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
              />
            </div>

            <div class="flex flex-col gap-2">
              <label class="flex cursor-pointer items-center gap-2 pt-5 font-mono text-xs text-ink">
                <input
                  type="checkbox"
                  :checked="participant.isWinner"
                  @change="setWinner(index)"
                />
                <span>Winner</span>
              </label>
            </div>
          </div>
        </div>
      </div>

      <!-- Bans -->
      <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
        <h4 class="headline text-base text-cyan">Bans (optional)</h4>

        <div
          v-for="(ban, index) in form.bans"
          :key="index"
          class="flex flex-col gap-4 border border-edge bg-surface-low p-4 sm:flex-row sm:items-start"
        >
          <div class="grid flex-1 grid-cols-2 gap-4 sm:grid-cols-4">
            <div class="flex flex-col gap-2">
              <label :for="`ban-hero-${index}`" class="label-caps">Hero</label>
              <select
                :id="`ban-hero-${index}`"
                v-model.number="ban.heroId"
                class="border border-edge bg-surface-lowest px-2 py-1.5 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
              >
                <option :value="0" disabled>Select a hero…</option>
                <option v-for="hero in heroPool" :key="hero.id" :value="hero.id">{{ hero.name }}</option>
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
