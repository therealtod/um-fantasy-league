<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import StatusBadge from '@/components/StatusBadge.vue'
import { useAuthStore } from '@/stores/auth'
import { useRosterStore } from '@/stores/roster'
import { useTournamentsStore } from '@/stores/tournaments'
import type { Tournament } from '@/api/types'

const router = useRouter()
const tournamentsStore = useTournamentsStore()
const rosterStore = useRosterStore()
const authStore = useAuthStore()

const tournaments = computed(() => tournamentsStore.tournaments)

function formatCredits(value: number) {
  return value.toLocaleString('en-US')
}

function enrolmentPercent(tournament: Tournament) {
  return Math.min(100, Math.round((tournament.enrolled / tournament.capacity) * 100))
}

const DATE_FORMAT = new Intl.DateTimeFormat('en-GB', {
  day: '2-digit',
  month: 'short',
  year: 'numeric',
  timeZone: 'UTC',
})

/** `start_date`/`end_date` are plain dates; parse as UTC so they never shift a day. */
function day(value: string) {
  const parsed = new Date(`${value.slice(0, 10)}T00:00:00Z`)
  return Number.isNaN(parsed.getTime()) ? value : DATE_FORMAT.format(parsed)
}

function dateRange(tournament: Tournament) {
  const start = day(tournament.startDate)
  if (!tournament.endDate) return start
  const end = day(tournament.endDate)
  return end === start ? start : `${start} – ${end}`
}

async function register(tournament: Tournament) {
  if (!authStore.isAuthenticated) {
    void router.push({ name: 'login', query: { redirect: '/lobby' } })
    return
  }
  const ok = await tournamentsStore.register(tournament.id)
  if (ok) {
    await rosterStore.select(tournament.id)
    void router.push(`/tournaments/${tournament.id}/roster`)
  }
}

function openRoster(tournament: Tournament) {
  void rosterStore.select(tournament.id)
  void router.push(`/tournaments/${tournament.id}/roster`)
}

function openStandings() {
  void router.push('/standings')
}
</script>

<template>
  <div class="mx-auto max-w-6xl">

    <div class="mt-8 flex items-baseline justify-between">
      <h3 class="label-caps">Currently available tournaments</h3>
      <span class="label-caps">{{ tournaments.length }} listed</span>
    </div>

    <p v-if="tournamentsStore.loading" class="mt-6 font-mono text-sm text-ink-dim">
      Loading tournaments…
    </p>

    <p v-else-if="tournamentsStore.error" class="mt-6 border border-magenta/50 bg-magenta/10 p-4 font-mono text-sm text-magenta">
      {{ tournamentsStore.error }}
    </p>

    <ul v-else class="mt-4 space-y-3">
      <li
        v-for="tournament in tournaments"
        :key="tournament.id"
        class="panel p-5 transition-colors hover:border-edge-strong"
        :class="{ 'glow-cyan': tournament.status === 'LIVE' }"
      >
        <div class="flex flex-wrap items-start justify-between gap-6">
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-3">
              <StatusBadge :status="tournament.status" />
              <span class="label-caps">{{ tournament.format }}</span>
            </div>

            <h4 class="headline mt-3 text-xl uppercase">{{ tournament.name }}</h4>

            <div class="mt-3 flex flex-wrap items-center gap-x-6 gap-y-2 font-mono text-xs text-ink-dim">
              <span>{{ dateRange(tournament) }}</span>
              <span>Roster size: {{ tournament.rosterSize }}</span>
            </div>

            <!-- Enrolment -->
            <div class="mt-4 max-w-md">
              <div class="flex items-baseline justify-between">
                <span class="label-caps">Enrolled</span>
                <span class="stat-value text-xs text-ink-muted">
                  {{ tournament.enrolled }}/{{ tournament.capacity }}
                </span>
              </div>
              <div class="mt-1.5 h-1 w-full bg-surface-high">
                <div
                  class="h-full bg-cyan transition-[width] duration-300"
                  :style="{ width: `${enrolmentPercent(tournament)}%` }"
                />
              </div>
            </div>
          </div>

          <!-- Budget + action -->
          <div
            class="flex w-full shrink-0 flex-wrap items-center justify-between gap-4 sm:w-auto sm:justify-end sm:gap-6"
          >
            <div class="text-left sm:text-right">
              <p class="label-caps">Credit Grant</p>
              <p class="stat-value mt-1 text-base text-cyan">
                {{ formatCredits(tournament.creditGrant) }} <span class="text-ink-dim">CR</span>
              </p>
            </div>

            <div class="w-full sm:w-44">
              <button
                v-if="!tournament.myEntryStatus && tournament.acceptsRegistration"
                class="btn-primary w-full"
                :disabled="tournamentsStore.registering === tournament.id"
                @click="register(tournament)"
              >
                <template v-if="tournamentsStore.registering === tournament.id">Registering…</template>
                <template v-else>Register</template>
              </button>

              <button
                v-else-if="tournament.myEntryStatus === 'DRAFT'"
                class="btn-primary w-full"
                @click="openRoster(tournament)"
              >
                Build Roster
              </button>

              <button
                v-else-if="tournament.myEntryStatus === 'LOCKED'"
                class="btn-ghost w-full"
                @click="tournament.status === 'LIVE' ? openStandings() : openRoster(tournament)"
              >
                {{ tournament.status === 'LIVE' ? 'Enter Spectator' : 'Roster Locked' }}
              </button>

              <button v-else class="btn-ghost w-full" disabled>
                {{ tournament.status === 'LIVE' ? 'In Progress' : 'Not Open' }}
              </button>
            </div>
          </div>
        </div>
      </li>
    </ul>
  </div>
</template>
