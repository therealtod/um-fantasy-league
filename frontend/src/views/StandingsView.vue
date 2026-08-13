<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from 'vue'
import { useManagerStore } from '@/stores/manager'
import { useStandingsStore } from '@/stores/standings'
import { useTournamentsStore } from '@/stores/tournaments'
import type { MetricColumn } from '@/api/types'

const standings = useStandingsStore()
const tournaments = useTournamentsStore()
const managerStore = useManagerStore()

/** Standings follow the live tournament; fall back to the first listed. */
const options = computed(() =>
  tournaments.tournaments.filter((t) => t.status === 'LIVE' || t.status === 'COMPLETED'),
)

function defaultTournamentId() {
  return (tournaments.live[0] ?? options.value[0] ?? tournaments.tournaments[0])?.id ?? null
}

function start(id: number | null) {
  if (id === null) return
  void standings.load(id)
}

onMounted(() => start(standings.tournamentId ?? defaultTournamentId()))
onUnmounted(() => standings.stop())

watch(
  () => tournaments.tournaments,
  (list) => {
    if (standings.tournamentId === null && list.length > 0) start(defaultTournamentId())
  },
)

function onTournamentChange(event: Event) {
  const value = (event.target as HTMLSelectElement).value
  if (value) start(Number(value))
}

function points(value: number) {
  return `${value > 0 ? '+' : ''}${value.toFixed(1)}`
}

const isMe = (managerId: number) => managerStore.manager?.id === managerId

const board = computed(() => standings.board)
/** The leaderboard's columns are whatever the active rule set defines. */
const metrics = computed<MetricColumn[]>(() => board.value?.metrics ?? [])

/** A penalty column, or a negative total, reads magenta; a gain reads lime. */
function metricClass(value: number, column: MetricColumn) {
  if (value === 0) return 'text-ink-muted'
  if (value < 0 || column.coefficient < 0) return 'text-magenta'
  return 'text-lime'
}

const currentTournament = computed(() =>
  standings.tournamentId === null ? null : tournaments.byId(standings.tournamentId),
)
</script>

<template>
  <div>
    <!-- Ticker -->
    <div class="panel scanline-bg overflow-hidden">
      <div class="flex items-stretch">
        <div class="flex shrink-0 items-center border-r border-edge bg-surface-lowest px-4">
          <span class="label-caps text-cyan">Results</span>
        </div>
        <div class="min-w-0 flex-1 overflow-x-auto">
          <ul v-if="standings.ticker.length" class="flex items-center gap-8 px-4 py-3 whitespace-nowrap">
            <li
              v-for="entry in standings.ticker"
              :key="entry.matchId"
              class="font-mono text-xs text-ink-muted"
            >
              <span class="text-ink-dim">R{{ entry.round }}:</span>
              <template v-for="(game, gameIndex) in entry.games" :key="game.gameNumber">
                <span v-if="gameIndex > 0" class="mx-1.5 text-ink-dim">|</span>
                <span v-if="entry.games.length > 1" class="text-ink-dim">G{{ game.gameNumber }}</span>
                <template v-for="(side, sideIndex) in game.sides" :key="side.heroName">
                  <!-- Sides come winner-first and every game has one, so this always reads "winner def loser". -->
                  <span v-if="sideIndex > 0" class="mx-1 text-ink-dim">def</span>
                  <span
                    class="font-bold uppercase"
                    :class="side.isWinner ? 'text-ink' : 'text-ink-muted'"
                  >{{ side.heroName }}</span>
                  <span v-if="side.playerLabel" class="ml-1 text-ink-dim">({{ side.playerLabel }})</span>
                  <span
                    class="ml-1 font-bold"
                    :class="side.points < 0 ? 'text-magenta' : 'text-lime'"
                  >{{ points(side.points) }}</span>
                </template>
                <span class="ml-1.5 text-ink-dim">· {{ game.mapName }}</span>
              </template>
              <span v-if="entry.bannedHeroNames.length" class="ml-1.5 text-ink-dim">
                · Banned: {{ entry.bannedHeroNames.join(', ') }}
              </span>
            </li>
          </ul>
          <p v-else class="px-4 py-3 font-mono text-xs text-ink-dim">
            No recorded results yet.
          </p>
        </div>
      </div>
    </div>

    <!-- Controls -->
    <div class="mt-6 flex flex-wrap items-center justify-between gap-4">
      <div class="flex items-center gap-3">
        <label for="standings-tournament" class="label-caps">Deployment</label>
        <select
          id="standings-tournament"
          class="field-input text-xs"
          :value="standings.tournamentId ?? ''"
          @change="onTournamentChange"
        >
          <option v-for="tournament in tournaments.tournaments" :key="tournament.id" :value="tournament.id">
            {{ tournament.name }}
          </option>
        </select>
      </div>

      <p v-if="currentTournament" class="label-caps">
        {{ currentTournament.status }}
        <template v-if="board"> · {{ board.ruleSetName }} · Round {{ board.currentRound }}</template>
      </p>
    </div>

    <p
      v-if="standings.error"
      class="mt-4 border border-magenta/50 bg-magenta/10 p-3 font-mono text-xs text-magenta"
    >
      {{ standings.error }}
    </p>

    <!-- Leaderboard -->
    <div class="panel mt-4 overflow-x-auto">
      <table class="w-max min-w-full border-collapse">
        <thead>
          <tr class="border-b border-edge bg-surface-lowest text-left">
            <!-- `w-12` only holds if content + padding stays under 48px, and the
                 Manager column's `left-12` offset depends on it exactly. Hence px-3. -->
            <th class="label-caps sticky left-0 z-20 w-12 bg-surface-lowest px-3 py-3">Rnk</th>
            <th
              class="label-caps cell-pinned-edge sticky left-12 z-20 bg-surface-lowest px-4 py-3 whitespace-nowrap"
            >
              Manager
            </th>
            <th class="label-caps px-4 py-3 whitespace-nowrap">Roster</th>
            <th
              v-for="metric in metrics"
              :key="metric.metric"
              class="label-caps px-4 py-3 text-right whitespace-nowrap"
              :title="`${metric.metric} × ${metric.coefficient}`"
            >
              {{ metric.label }}
            </th>
            <th class="label-caps px-4 py-3 text-right whitespace-nowrap">Last Rd</th>
            <th class="label-caps px-4 py-3 text-right whitespace-nowrap">Total Pts</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="standings.loading">
            <td :colspan="5 + metrics.length" class="px-4 py-6 font-mono text-sm text-ink-dim">
              Loading standings…
            </td>
          </tr>
          <tr v-else-if="standings.rows.length === 0">
            <td :colspan="5 + metrics.length" class="px-4 py-6 font-mono text-sm text-ink-dim">
              No entries in this tournament yet.
            </td>
          </tr>
          <tr
            v-for="row in standings.rows"
            :key="row.entryId"
            class="border-b border-edge last:border-b-0"
            :class="isMe(row.managerId) ? 'row-opaque-mine' : 'row-opaque'"
          >
            <td
              class="stat-value cell-pinned left-0 px-3 py-3 text-sm"
              :class="isMe(row.managerId) ? 'text-magenta' : 'text-ink-dim'"
            >
              {{ row.rank }}
            </td>
            <td class="cell-pinned cell-pinned-edge left-12 px-4 py-3">
              <!-- Capped: this column is pinned, so an unusually long display
                   name would otherwise eat the width the metrics scroll into. -->
              <p
                class="max-w-[9rem] truncate font-mono text-xs font-bold uppercase"
                :class="isMe(row.managerId) ? 'text-magenta' : 'text-ink'"
              >
                {{ row.handle }}
              </p>
              <p class="max-w-[9rem] truncate font-mono text-[10px] text-ink-dim">
                {{ row.displayName }}
              </p>
            </td>
            <td class="min-w-[12rem] px-4 py-3 font-mono text-[11px] text-ink-muted">
              {{ row.roster.length ? row.roster.join(' · ') : '—' }}
            </td>
            <td
              v-for="metric in metrics"
              :key="metric.metric"
              class="stat-value px-4 py-3 text-right text-xs whitespace-nowrap"
              :class="metricClass(row.breakdown[metric.metric] ?? 0, metric)"
            >
              {{ points(row.breakdown[metric.metric] ?? 0) }}
            </td>
            <td class="stat-value px-4 py-3 text-right text-xs whitespace-nowrap text-cyan">
              {{ points(row.roundPoints) }}
            </td>
            <td class="stat-value px-4 py-3 text-right text-sm whitespace-nowrap text-ink">
              {{ row.totalPoints.toFixed(1) }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- The 6px scrollbar barely registers, so name the gesture. -->
    <p class="label-caps mt-2 md:hidden">Scroll → for the full breakdown</p>
  </div>
</template>
