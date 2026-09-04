<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from 'vue'
import { useManagerStore } from '@/stores/manager'
import { useStandingsStore } from '@/stores/standings'
import { useTournamentsStore } from '@/stores/tournaments'
import type { MetricColumn } from '@/api/types'
import ErrorBanner from '@/components/ErrorBanner.vue'

const standings = useStandingsStore()
const tournaments = useTournamentsStore()
const managerStore = useManagerStore()

/** Standings follow the live tournament; fall back to the first listed. */
const options = computed(() =>
  tournaments.tournaments.filter((t) => t.status === 'LIVE' || t.status === 'COMPLETED'),
)

function defaultTournamentId() {
  return (tournaments.live[0] ?? options.value[0])?.id ?? null
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
              <!-- Drafted and never fielded: they appear in no game row above, but
                   they scored an appearance, so the ticker has to name them. -->
              <span v-if="entry.draftedUnplayedHeroNames.length" class="ml-1.5 text-ink-dim">
                · Drafted, unplayed: {{ entry.draftedUnplayedHeroNames.join(', ') }}
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
          <option v-if="options.length === 0" value="" disabled>No live tournaments yet</option>
          <option v-for="tournament in options" :key="tournament.id" :value="tournament.id">
            {{ tournament.name }}
          </option>
        </select>
      </div>

      <p v-if="currentTournament" class="label-caps">
        {{ currentTournament.status }}
        <template v-if="board"> · {{ board.ruleSetName }} · Round {{ board.currentRound }}</template>
      </p>
    </div>

    <ErrorBanner v-if="standings.error" class="mt-4" compact :message="standings.error" />

    <!-- Leaderboard -->
    <div v-if="standings.tournamentId !== null" class="panel mt-4 overflow-x-auto">
      <table class="w-max min-w-full border-collapse">
        <thead>
          <tr class="border-b border-edge bg-surface-lowest text-left">
            <!-- Width and offset both come from `--pinned-rank-width`; the padding
                 is what keeps a two-digit rank inside it at the phone width. See
                 `.cell-pinned-rank` in main.css. -->
            <th class="label-caps cell-pinned-rank sticky z-20 bg-surface-lowest px-2 py-3 md:px-3">Rnk</th>
            <th
              class="label-caps cell-pinned-manager sticky z-20 bg-surface-lowest px-3 py-3 whitespace-nowrap md:px-4"
            >
              Manager
            </th>
            <!-- The headline number, pinned beside the identity: on a phone the
                 breakdown scrolls away underneath it, the total never does. -->
            <th
              class="label-caps cell-pinned-edge cell-pinned-total cell-total-emphasis sticky z-20 bg-surface-lowest px-3 py-3 text-right text-cyan whitespace-nowrap"
              title="Total points · last round below"
            >
              Total
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
          </tr>
        </thead>
        <tbody>
          <tr v-if="standings.loading">
            <td :colspan="4 + metrics.length" class="px-4 py-6 font-mono text-sm text-ink-dim">
              Loading standings…
            </td>
          </tr>
          <tr v-else-if="standings.rows.length === 0">
            <td :colspan="4 + metrics.length" class="px-4 py-6 font-mono text-sm text-ink-dim">
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
              class="stat-value cell-pinned cell-pinned-rank px-2 py-3 text-sm md:px-3"
              :class="isMe(row.managerId) ? 'text-magenta' : 'text-ink-dim'"
            >
              {{ row.rank }}
            </td>
            <td class="cell-pinned cell-pinned-manager px-3 py-3 md:px-4">
              <!-- Capped: this column is pinned, so an unusually long display
                   name would otherwise eat the width the metrics scroll into —
                   and push the total's pin offset off the column boundary. -->
              <p
                class="max-w-[6rem] truncate font-mono text-xs font-bold uppercase md:max-w-[9rem]"
                :class="isMe(row.managerId) ? 'text-magenta' : 'text-ink'"
              >
                {{ row.handle }}
              </p>
              <p class="max-w-[6rem] truncate font-mono text-[10px] text-ink-dim md:max-w-[9rem]">
                {{ row.displayName }}
              </p>
            </td>
            <td
              class="cell-pinned cell-pinned-edge cell-pinned-total cell-total-emphasis px-3 py-3 text-right"
            >
              <p
                class="stat-value text-base whitespace-nowrap md:text-lg"
                :class="isMe(row.managerId) ? 'text-magenta' : 'text-ink'"
              >
                {{ row.totalPoints.toFixed(1) }}
              </p>
              <!-- The round's gain rides under the total rather than in a column
                   of its own: the two numbers are read together, and a phone has
                   no width to spare. -->
              <p class="stat-value mt-1 text-[10px] whitespace-nowrap text-cyan">
                {{ points(row.roundPoints) }} rd
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
          </tr>
        </tbody>
      </table>
    </div>
    <div v-else class="panel mt-4 px-4 py-6 font-mono text-sm text-ink-dim">
      No live or completed tournaments yet — standings appear once a tournament goes live.
    </div>

    <!-- The 6px scrollbar barely registers, so name the gesture. -->
    <p v-if="standings.tournamentId !== null" class="label-caps mt-2 md:hidden">
      Scroll → for the full breakdown
    </p>
  </div>
</template>
