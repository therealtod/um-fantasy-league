<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useHeroStatsStore } from '@/stores/heroStats'
import { useTournamentsStore } from '@/stores/tournaments'
import type { HeroRankRow } from '@/api/types'
import { standingsAvailable } from '@/domain/tournamentStatus'
import {
  OVERALL,
  TOP_ROWS,
  hasHiddenRows,
  heroMatrix,
  nextSort,
  pointsTone,
  sortMatrix,
  visibleRows,
  type MatrixSort,
  type PointsTone,
} from '@/domain/heroStats'
import { formatCredits, formatPoints } from '@/lib/format'
import ErrorBanner from '@/components/ErrorBanner.vue'

const heroStats = useHeroStatsStore()
const tournaments = useTournamentsStore()

/** The same tournaments the leaderboard offers: only those that can have results. */
const options = computed(() => tournaments.tournaments.filter((t) => standingsAvailable(t.status)))

function defaultTournamentId() {
  return (tournaments.live[0] ?? options.value[0])?.id ?? null
}

/** Metric keys whose table is showing every hero rather than the top ten. */
const expanded = ref(new Set<string>())

const DEFAULT_SORT: MatrixSort = { key: OVERALL, direction: 'desc' }

/** The all-heroes table's column and direction; another tournament may not price the same metrics. */
const sort = ref<MatrixSort>(DEFAULT_SORT)

function start(id: number | null) {
  if (id === null) return
  expanded.value = new Set()
  sort.value = DEFAULT_SORT
  void heroStats.load(id)
}

onMounted(() => start(heroStats.tournamentId ?? defaultTournamentId()))
onUnmounted(() => heroStats.stop())

watch(
  () => tournaments.tournaments,
  (list) => {
    if (heroStats.tournamentId === null && list.length > 0) start(defaultTournamentId())
  },
)

function onTournamentChange(event: Event) {
  const value = (event.target as HTMLSelectElement).value
  if (value) start(Number(value))
}

const board = computed(() => heroStats.board)

interface HeroTable {
  key: string
  label: string
  caption: string
  /** Drives the penalty colouring; the overall table has no single weight. */
  coefficient: number
  rows: HeroRankRow[]
  overall: boolean
}

/** Overall first, then one table per criterion in the rule set's own order. */
const tables = computed<HeroTable[]>(() => {
  if (!board.value) return []
  return [
    {
      key: OVERALL,
      label: 'Overall',
      caption: 'All criteria',
      coefficient: 1,
      rows: board.value.overall,
      overall: true,
    },
    ...board.value.categories.map((category) => ({
      key: category.metric,
      label: category.label,
      caption: `× ${category.coefficient}`,
      coefficient: category.coefficient,
      rows: category.rows,
      overall: false,
    })),
  ]
})

interface MatrixColumn {
  key: string
  label: string
  title: string
  coefficient: number
}

/** The all-heroes table's sortable columns: the total, then each criterion in the rule set's order. */
const matrixColumns = computed<MatrixColumn[]>(() => {
  if (!board.value) return []
  return [
    { key: OVERALL, label: 'Overall', title: 'All criteria', coefficient: 1 },
    ...board.value.categories.map((category) => ({
      key: category.metric,
      label: category.label,
      title: `${category.metric} × ${category.coefficient}`,
      coefficient: category.coefficient,
    })),
  ]
})

/** The criteria columns that scroll; Overall is pinned beside the hero instead. */
const metricColumns = computed(() => matrixColumns.value.slice(1))

/** The sort actually applied: a column a live refresh dropped falls back to Overall. */
const activeSort = computed<MatrixSort>(() =>
  matrixColumns.value.some((column) => column.key === sort.value.key) ? sort.value : DEFAULT_SORT,
)

const matrixRows = computed(() => (board.value ? sortMatrix(heroMatrix(board.value), activeSort.value) : []))

function sortBy(key: string) {
  sort.value = nextSort(activeSort.value, key)
}

function ariaSort(key: string) {
  if (activeSort.value.key !== key) return 'none'
  return activeSort.value.direction === 'desc' ? 'descending' : 'ascending'
}

function sortGlyph(key: string) {
  if (activeSort.value.key !== key) return ''
  return activeSort.value.direction === 'desc' ? '▼' : '▲'
}

function toggle(key: string) {
  const next = new Set(expanded.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  expanded.value = next
}

const TONE_CLASS: Record<PointsTone, string> = {
  muted: 'text-ink-muted',
  penalty: 'text-magenta',
  gain: 'text-lime',
}

const currentTournament = computed(() =>
  heroStats.tournamentId === null ? null : tournaments.byId(heroStats.tournamentId),
)
</script>

<template>
  <div>
    <!-- Controls -->
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div class="flex items-center gap-3">
        <label for="hero-stats-tournament" class="label-caps">Deployment</label>
        <select
          id="hero-stats-tournament"
          class="field-input text-xs"
          :value="heroStats.tournamentId ?? ''"
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
        <template v-if="board">
          <template v-if="board.ruleSetName"> · {{ board.ruleSetName }}</template>
          · Round {{ board.currentRound }}
        </template>
      </p>
    </div>

    <ErrorBanner v-if="heroStats.error" class="mt-4" compact :message="heroStats.error" />

    <div
      v-if="heroStats.tournamentId === null"
      class="panel mt-4 px-4 py-6 font-mono text-sm text-ink-dim"
    >
      No live or completed tournaments yet — hero performance appears once a tournament goes live.
    </div>

    <div
      v-else-if="heroStats.loading && !board"
      class="panel mt-4 px-4 py-6 font-mono text-sm text-ink-dim"
    >
      Loading hero performance…
    </div>

    <template v-else-if="board">
      <p
        v-if="board.categories.length === 0"
        class="panel mt-4 px-4 py-3 font-mono text-xs text-ink-dim"
      >
        This tournament has no active scoring rules, so there are no criteria to rank by.
      </p>
      <p
        v-else-if="board.currentRound === 0"
        class="panel mt-4 px-4 py-3 font-mono text-xs text-ink-dim"
      >
        No recorded results yet — every hero is still on zero.
      </p>

      <!-- Every hero against every criterion at once, sorted by whichever header
           was clicked. Same scroll shape as the standings leaderboard: rank, hero
           and overall stay pinned while the criteria scroll underneath. -->
      <section
        v-if="board.overall.length > 0"
        class="panel mt-4 min-w-0"
        aria-labelledby="hero-matrix-title"
      >
        <header
          class="flex items-baseline justify-between gap-3 border-b border-edge bg-surface-lowest px-4 py-3"
        >
          <h2 id="hero-matrix-title" class="label-caps text-cyan">All heroes</h2>
          <span class="label-caps">Click a column to sort</span>
        </header>
        <div class="overflow-x-auto">
          <table id="hero-matrix" class="w-max min-w-full border-collapse">
            <thead>
              <tr class="border-b border-edge bg-surface-lowest text-left">
                <th
                  scope="col"
                  class="label-caps cell-pinned-rank sticky z-20 bg-surface-lowest px-2 py-3 md:px-3"
                >
                  Rnk
                </th>
                <!-- The hero rides the leaderboard's manager pin: same width, same
                     offset chain, so the overall column pins right after it. -->
                <th
                  scope="col"
                  class="label-caps cell-pinned-manager sticky z-20 bg-surface-lowest px-3 py-3 whitespace-nowrap md:px-4"
                >
                  Hero
                </th>
                <th
                  scope="col"
                  class="cell-pinned-edge cell-pinned-total cell-total-emphasis sticky z-20 bg-surface-lowest p-0 text-right"
                  :aria-sort="ariaSort(OVERALL)"
                >
                  <button
                    type="button"
                    class="label-caps w-full px-3 py-3 text-right whitespace-nowrap text-cyan"
                    title="All criteria"
                    @click="sortBy(OVERALL)"
                  >
                    Overall {{ sortGlyph(OVERALL) }}
                  </button>
                </th>
                <th
                  v-for="column in metricColumns"
                  :key="column.key"
                  scope="col"
                  class="p-0 text-right"
                  :aria-sort="ariaSort(column.key)"
                >
                  <button
                    type="button"
                    class="label-caps w-full px-4 py-3 text-right whitespace-nowrap hover:text-ink"
                    :class="{ 'text-cyan': activeSort.key === column.key }"
                    :title="column.title"
                    @click="sortBy(column.key)"
                  >
                    {{ column.label }} {{ sortGlyph(column.key) }}
                  </button>
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="row in matrixRows"
                :key="row.heroId"
                class="row-opaque border-b border-edge last:border-b-0"
              >
                <!-- The rank for the sorted column, as that column's own table ranks it. -->
                <td class="stat-value cell-pinned cell-pinned-rank px-2 py-2 text-sm text-ink-dim md:px-3">
                  {{ row.ranks[activeSort.key] ?? '—' }}
                </td>
                <td class="cell-pinned cell-pinned-manager px-3 py-2 md:px-4">
                  <p class="max-w-[6rem] truncate font-mono text-xs font-bold text-ink uppercase md:max-w-[9rem]">
                    {{ row.heroName }}
                  </p>
                  <p class="max-w-[6rem] truncate font-mono text-[10px] text-ink-dim md:max-w-[9rem]">
                    {{ formatCredits(row.cost) }}
                  </p>
                </td>
                <td
                  class="stat-value cell-pinned cell-pinned-edge cell-pinned-total cell-total-emphasis px-3 py-2 text-right text-sm whitespace-nowrap"
                  :class="TONE_CLASS[pointsTone(row.points[OVERALL] ?? 0)]"
                >
                  {{ formatPoints(row.points[OVERALL] ?? 0) }}
                </td>
                <td
                  v-for="column in metricColumns"
                  :key="column.key"
                  class="stat-value px-4 py-2 text-right text-xs whitespace-nowrap"
                  :class="TONE_CLASS[pointsTone(row.points[column.key] ?? 0, column.coefficient)]"
                >
                  {{ formatPoints(row.points[column.key] ?? 0) }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      <div class="mt-4 grid gap-4 lg:grid-cols-2 2xl:grid-cols-3">
        <section
          v-for="table in tables"
          :key="table.key"
          class="panel flex min-w-0 flex-col"
          :aria-labelledby="`hero-table-${table.key}`"
        >
          <header
            class="flex items-baseline justify-between gap-3 border-b border-edge bg-surface-lowest px-4 py-3"
          >
            <h2
              :id="`hero-table-${table.key}`"
              class="label-caps"
              :class="table.overall ? 'text-cyan' : 'text-ink'"
            >
              {{ table.label }}
            </h2>
            <span class="label-caps" :title="table.overall ? undefined : `${table.key} × ${table.coefficient}`">
              {{ table.caption }}
            </span>
          </header>

          <p v-if="table.rows.length === 0" class="px-4 py-4 font-mono text-xs text-ink-dim">
            No heroes in this tournament's pool.
          </p>
          <!-- Fixed layout: the name column absorbs whatever the rank and points
               columns leave, and truncates rather than widening the card. -->
          <table v-else :id="`hero-rows-${table.key}`" class="w-full table-fixed border-collapse">
            <thead>
              <tr class="border-b border-edge text-left">
                <th scope="col" class="label-caps w-12 px-3 py-2">Rnk</th>
                <th scope="col" class="label-caps px-2 py-2">Hero</th>
                <th scope="col" class="label-caps w-20 px-3 py-2 text-right">Pts</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="row in visibleRows(table.rows, expanded.has(table.key))"
                :key="row.heroId"
                class="border-b border-edge last:border-b-0"
              >
                <td class="stat-value px-3 py-2 text-sm text-ink-dim">{{ row.rank }}</td>
                <td class="px-2 py-2">
                  <p class="truncate font-mono text-xs font-bold text-ink uppercase">{{ row.heroName }}</p>
                  <p class="truncate font-mono text-[10px] text-ink-dim">{{ formatCredits(row.cost) }}</p>
                </td>
                <td
                  class="stat-value px-3 py-2 text-right text-xs whitespace-nowrap"
                  :class="TONE_CLASS[pointsTone(row.points, table.coefficient)]"
                >
                  {{ formatPoints(row.points) }}
                </td>
              </tr>
            </tbody>
          </table>

          <button
            v-if="hasHiddenRows(table.rows)"
            type="button"
            class="btn-ghost mt-auto w-full border-x-0 border-b-0"
            :aria-expanded="expanded.has(table.key)"
            :aria-controls="`hero-rows-${table.key}`"
            @click="toggle(table.key)"
          >
            {{ expanded.has(table.key) ? `Show top ${TOP_ROWS}` : `Show all ${table.rows.length}` }}
          </button>
        </section>
      </div>
    </template>
  </div>
</template>
