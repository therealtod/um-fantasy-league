<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useHeroStatsStore } from '@/stores/heroStats'
import { useTournamentsStore } from '@/stores/tournaments'
import type { HeroRankRow } from '@/api/types'
import { standingsAvailable } from '@/domain/tournamentStatus'
import { TOP_ROWS, hasHiddenRows, pointsTone, visibleRows, type PointsTone } from '@/domain/heroStats'
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

function start(id: number | null) {
  if (id === null) return
  expanded.value = new Set()
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
      key: 'OVERALL',
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
