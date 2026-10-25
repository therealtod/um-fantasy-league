<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useHallOfFameStore } from '@/stores/hallOfFame'
import { useManagerStore } from '@/stores/manager'
import ErrorBanner from '@/components/ErrorBanner.vue'
import { formatDateRange } from '@/lib/format'

const store = useHallOfFameStore()
const managerStore = useManagerStore()

// Reloaded on every visit: there is no stream here, so this is what picks up a
// result corrected after a tournament finished.
onMounted(() => void store.load())

const titles = computed(() => store.hallOfFame?.titles ?? [])
const tournaments = computed(() => store.hallOfFame?.tournaments ?? [])

const isMe = (managerId: number) => managerStore.manager?.id === managerId

/** Ranks, not positions: a tie for second is two "2nd"s and no "3rd". */
const PLACE_LABELS: Record<number, string> = { 1: 'Champion', 2: '2nd', 3: '3rd' }
const place = (rank: number) => PLACE_LABELS[rank] ?? `${rank}th`
</script>

<template>
  <div class="space-y-8">
    <ErrorBanner v-if="store.error" compact :message="store.error" />

    <p v-if="store.loading && !store.hallOfFame" class="panel px-4 py-6 font-mono text-sm text-ink-dim">
      Loading the Hall of Fame…
    </p>

    <p
      v-else-if="store.hallOfFame && tournaments.length === 0"
      id="hall-of-fame-empty"
      class="panel px-4 py-6 font-mono text-sm text-ink-dim"
    >
      No tournament has finished yet — champions appear here once one is completed.
    </p>

    <template v-else-if="store.hallOfFame">
      <!-- All-time titles -->
      <section v-if="titles.length">
        <h2 class="label-caps mb-3 text-cyan">All-time Titles</h2>
        <div class="panel overflow-x-auto">
          <table id="hall-of-fame-titles" class="w-full border-collapse">
            <thead>
              <tr class="border-b border-edge bg-surface-lowest text-left">
                <th class="label-caps px-3 py-3">Rnk</th>
                <th class="label-caps px-3 py-3">Manager</th>
                <th class="label-caps px-3 py-3 text-right text-lime">Titles</th>
                <th class="label-caps px-3 py-3 text-right">Podiums</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="row in titles"
                :key="row.managerId"
                class="border-b border-edge last:border-b-0"
              >
                <td class="stat-value px-3 py-3 text-sm text-ink-dim">{{ row.rank }}</td>
                <td class="px-3 py-3">
                  <p
                    class="font-mono text-xs font-bold uppercase"
                    :class="isMe(row.managerId) ? 'text-magenta' : 'text-ink'"
                  >
                    {{ row.handle }}
                  </p>
                  <p class="font-mono text-[10px] text-ink-dim">{{ row.displayName }}</p>
                </td>
                <td
                  class="stat-value px-3 py-3 text-right text-base"
                  :class="row.titles > 0 ? 'text-lime' : 'text-ink-muted'"
                >
                  {{ row.titles }}
                </td>
                <td class="stat-value px-3 py-3 text-right text-sm text-ink-muted">{{ row.podiums }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      <!-- One card per completed tournament, newest first -->
      <section class="space-y-4">
        <h2 class="label-caps text-cyan">Past Tournaments</h2>
        <article
          v-for="tournament in tournaments"
          :key="tournament.tournamentId"
          class="panel hall-of-fame-tournament"
        >
          <header class="flex flex-wrap items-baseline justify-between gap-2 border-b border-edge px-4 py-3">
            <h3 class="headline text-base uppercase">{{ tournament.name }}</h3>
            <p class="label-caps">
              {{ tournament.format }} · {{ formatDateRange(tournament.startDate, tournament.endDate) }}
              · {{ tournament.entrants }} {{ tournament.entrants === 1 ? 'manager' : 'managers' }}
            </p>
          </header>

          <p v-if="tournament.podium.length === 0" class="px-4 py-4 font-mono text-sm text-ink-dim">
            Nobody entered this tournament.
          </p>
          <ol v-else>
            <li
              v-for="row in tournament.podium"
              :key="row.entryId"
              class="podium-row flex items-start gap-3 border-b border-edge px-4 py-3 last:border-b-0"
              :class="row.rank === 1 ? 'bg-lime/10' : ''"
            >
              <span
                class="label-caps w-20 shrink-0 pt-0.5"
                :class="row.rank === 1 ? 'text-lime' : 'text-ink-dim'"
              >
                {{ place(row.rank) }}
              </span>
              <div class="min-w-0 flex-1">
                <p
                  class="truncate font-mono text-xs font-bold uppercase"
                  :class="isMe(row.managerId) ? 'text-magenta' : row.rank === 1 ? 'text-lime' : 'text-ink'"
                >
                  {{ row.handle }}
                </p>
                <p class="truncate font-mono text-[10px] text-ink-dim">{{ row.displayName }}</p>
                <p class="mt-1 font-mono text-[11px] break-words text-ink-muted">
                  {{ row.roster.length ? row.roster.join(' · ') : '—' }}
                </p>
              </div>
              <span
                class="stat-value shrink-0 text-right text-base md:text-lg"
                :class="row.rank === 1 ? 'text-lime' : 'text-ink'"
              >
                {{ row.totalPoints.toFixed(1) }}
              </span>
            </li>
          </ol>
          <p class="label-caps border-t border-edge px-4 py-2">Scored under {{ tournament.ruleSetName }}</p>
        </article>
      </section>
    </template>
  </div>
</template>
