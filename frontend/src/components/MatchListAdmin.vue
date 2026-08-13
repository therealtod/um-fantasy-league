<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import type { BanType, MatchResultDto } from '@/api/types'
import { api } from '@/api/client'
import ErrorBanner from '@/components/ErrorBanner.vue'
import DestructiveConfirmPanel from '@/components/DestructiveConfirmPanel.vue'

interface Props {
  tournamentId: number
}

const props = defineProps<Props>()

const matches = ref<MatchResultDto[]>([])
const isLoading = ref(false)
const error = ref<string | null>(null)
const selectedRound = ref<number | null>(null)
const deletingMatch = ref<MatchResultDto | null>(null)
const isDeleting = ref(false)

const banTypeLabels: Record<BanType, string> = {
  PRE_BAN: 'Pre-ban',
  OPPONENT_BAN: 'Opponent ban',
  SELF_BAN: 'Self ban',
}

const rounds = computed(() => {
  const roundsSet = new Set<number>()
  matches.value.forEach(match => roundsSet.add(match.round))
  return Array.from(roundsSet).sort((a, b) => a - b)
})

// The round filter is client-side only: the dropdown's options are derived from the loaded
// matches, so narrowing the request would leave it offering only the round already selected.
const filteredMatches = computed(() => {
  if (selectedRound.value === null) return matches.value
  return matches.value.filter(match => match.round === selectedRound.value)
})

async function loadMatches() {
  isLoading.value = true
  error.value = null
  try {
    matches.value = await api.admin.listMatches(props.tournamentId)
  } catch (err) {
    error.value = err instanceof Error ? err.message : 'Failed to load matches'
  } finally {
    isLoading.value = false
  }
}

function formatDateTime(isoString: string): string {
  const date = new Date(isoString)
  return date.toLocaleDateString() + ' ' + date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}

function sideLabel(match: MatchResultDto, side: number): string {
  return match.participants.find(p => p.side === side)?.playerLabel || `Side ${side + 1}`
}

/**
 * Games won per side, derived client-side from each game's own `isWinner`
 * flags — there is no stored series winner (nothing about "best of N" is
 * tracked), consistent with this app's rule that anything derivable is
 * derived at read time rather than stored.
 */
function gamesWon(match: MatchResultDto): [number, number] {
  let side0 = 0
  let side1 = 0
  for (const game of match.games) {
    const winner = game.participants.find(p => p.isWinner)
    if (winner?.side === 0) side0++
    else if (winner?.side === 1) side1++
  }
  return [side0, side1]
}

function handleEdit(matchId: number) {
  emit('edit', matchId)
}

function startDelete(match: MatchResultDto) {
  error.value = null
  deletingMatch.value = match
}

function cancelDelete() {
  deletingMatch.value = null
}

async function confirmDelete() {
  const match = deletingMatch.value
  if (!match) return

  isDeleting.value = true
  error.value = null
  try {
    await api.admin.deleteMatch(props.tournamentId, match.matchId)
    deletingMatch.value = null
    await loadMatches() // Refresh the list
  } catch (err) {
    error.value = err instanceof Error ? err.message : 'Failed to delete match'
  } finally {
    isDeleting.value = false
  }
}

function handleCreate() {
  emit('create')
}

const emit = defineEmits<{
  create: []
  edit: [matchId: number]
}>()

// Load on mount, and again whenever the parent switches tournament — the round filter is
// reset with it, since round numbers from the old tournament mean nothing in the new one.
watch(
  () => props.tournamentId,
  () => {
    selectedRound.value = null
    deletingMatch.value = null
    loadMatches()
  },
  { immediate: true },
)
</script>

<template>
  <div class="panel p-4 md:p-8">
    <div class="mb-8 flex flex-wrap items-start justify-between gap-3">
      <h2 class="headline text-2xl text-cyan">Match Results</h2>
      <div class="flex items-center gap-4">
        <div class="flex items-center gap-2">
          <label for="round-filter" class="font-mono text-sm text-ink-dim">Filter by Round:</label>
          <select
            id="round-filter"
            v-model="selectedRound"
            class="cursor-pointer field-input-sm py-1"
          >
            <option :value="null">All Rounds</option>
            <option v-for="round in rounds" :key="round" :value="round">
              Round {{ round }}
            </option>
          </select>
        </div>
        <button class="btn-primary" @click="handleCreate">
          + Record New Match
        </button>
      </div>
    </div>

    <ErrorBanner class="mb-4" :message="error" />

    <!-- Delete confirmation -->
    <DestructiveConfirmPanel
      v-if="deletingMatch"
      class="mb-4"
      title="Delete Match"
      confirm-label="Delete Match"
      busy-label="Deleting..."
      :busy="isDeleting"
      @cancel="cancelDelete"
      @confirm="confirmDelete"
    >
      Are you sure you want to delete the round {{ deletingMatch.round }} match between
      <strong>{{ sideLabel(deletingMatch, 0) }}</strong> and <strong>{{ sideLabel(deletingMatch, 1) }}</strong>
      ({{ deletingMatch.games.length }} game{{ deletingMatch.games.length === 1 ? '' : 's' }})? Its participants,
      games and bans go with it, and every standing derived from it is recomputed. This cannot be undone.
    </DestructiveConfirmPanel>

    <div v-if="isLoading" class="p-8 text-center font-mono text-ink-dim">
      Loading matches...
    </div>

    <div v-else-if="filteredMatches.length === 0" class="p-12 text-center text-ink-dim">
      <p>No matches found.</p>
      <button class="btn-primary mt-4" @click="handleCreate">
        Record the first match
      </button>
    </div>

    <div v-else class="overflow-x-auto">
      <table class="w-full min-w-[64rem] border-collapse">
        <thead>
          <tr>
            <th class="border-b-2 border-edge bg-surface-lowest px-3 py-3 text-left font-mono text-sm text-ink-dim uppercase tracking-wide md:px-4">Round</th>
            <th class="border-b-2 border-edge bg-surface-lowest px-3 py-3 text-left font-mono text-sm text-ink-dim uppercase tracking-wide md:px-4">Played At</th>
            <th class="border-b-2 border-edge bg-surface-lowest px-3 py-3 text-left font-mono text-sm text-ink-dim uppercase tracking-wide md:px-4">Series</th>
            <th class="border-b-2 border-edge bg-surface-lowest px-3 py-3 text-left font-mono text-sm text-ink-dim uppercase tracking-wide md:px-4">Games</th>
            <th class="border-b-2 border-edge bg-surface-lowest px-3 py-3 text-left font-mono text-sm text-ink-dim uppercase tracking-wide md:px-4">Bans</th>
            <th class="border-b-2 border-edge bg-surface-lowest px-3 py-3 text-left font-mono text-sm text-ink-dim uppercase tracking-wide md:px-4">Link</th>
            <th class="w-[180px] border-b-2 border-edge bg-surface-lowest px-3 py-3 text-left font-mono text-sm text-ink-dim uppercase tracking-wide md:px-4">Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="match in filteredMatches"
            :key="match.matchId"
            class="border-b border-edge last:border-none hover:bg-surface-mid"
          >
            <td class="px-3 py-3 align-top md:px-4">
              <span class="inline-block border border-edge bg-surface-lowest px-1.5 py-0.5 font-mono text-xs text-ink-dim uppercase tracking-wide">
                Rd {{ match.round }}
              </span>
            </td>
            <td class="px-3 py-3 align-top font-mono text-sm md:px-4">{{ formatDateTime(match.playedAt) }}</td>
            <td class="px-3 py-3 align-top md:px-4">
              <div class="flex flex-col gap-1 font-mono text-sm">
                <span :class="gamesWon(match)[0] > gamesWon(match)[1] ? 'font-bold text-lime' : 'text-ink-dim'">
                  {{ sideLabel(match, 0) }} — {{ gamesWon(match)[0] }}
                </span>
                <span :class="gamesWon(match)[1] > gamesWon(match)[0] ? 'font-bold text-lime' : 'text-ink-dim'">
                  {{ sideLabel(match, 1) }} — {{ gamesWon(match)[1] }}
                </span>
              </div>
            </td>
            <td class="px-3 py-3 align-top md:px-4">
              <div class="flex flex-col gap-3">
                <div v-for="game in match.games" :key="game.gameId" class="flex flex-col gap-1">
                  <div class="font-mono text-xs text-ink-dim uppercase tracking-wide">
                    G{{ game.gameNumber }} · {{ game.mapName }}
                  </div>
                  <div
                    v-for="participant in game.participants"
                    :key="participant.side"
                    class="flex items-center gap-2 font-mono text-sm"
                  >
                    {{ participant.heroName }}
                    <span class="text-xs text-danger">❤️ {{ participant.healthRemaining }}</span>
                    <span
                      v-if="participant.isWinner"
                      class="bg-lime px-1.5 py-0.5 font-mono text-[10px] text-surface-lowest uppercase tracking-wide"
                    >
                      WIN
                    </span>
                  </div>
                </div>
              </div>
            </td>
            <td class="px-3 py-3 align-top md:px-4">
              <div v-if="match.bans.length > 0" class="flex flex-col gap-1">
                <div v-for="ban in match.bans" :key="ban.heroId" class="font-mono text-sm text-ink-dim">
                  {{ ban.heroName }} <span class="text-xs">({{ banTypeLabels[ban.banType] }})</span>
                </div>
              </div>
              <span v-else class="text-ink-dim italic">—</span>
            </td>
            <td class="px-3 py-3 align-top md:px-4">
              <a
                v-if="match.externalLink"
                :href="match.externalLink"
                target="_blank"
                rel="noopener noreferrer"
                class="font-mono text-sm text-cyan underline hover:opacity-80"
              >
                View
              </a>
              <span v-else class="text-ink-dim italic">—</span>
            </td>
            <td class="px-3 py-3 align-top md:px-4">
              <div class="flex gap-2">
                <button
                  class="border border-lime px-3 py-1 font-mono text-xs text-lime transition-colors hover:bg-lime/10"
                  @click="handleEdit(match.matchId)"
                >
                  Edit
                </button>
                <button
                  class="border border-danger px-3 py-1 font-mono text-xs text-danger transition-colors hover:bg-danger/10"
                  @click="startDelete(match)"
                >
                  Delete
                </button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
