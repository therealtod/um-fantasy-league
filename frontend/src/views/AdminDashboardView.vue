<script setup lang="ts">
import { computed, ref } from 'vue'
import HeroManagementWizard from '@/components/HeroManagementWizard.vue'
import MapManagementWizard from '@/components/MapManagementWizard.vue'
import TournamentManagementWizard from '@/components/TournamentManagementWizard.vue'
import HeroPoolWizard from '@/components/HeroPoolWizard.vue'
import MapPoolWizard from '@/components/MapPoolWizard.vue'
import ScoringRuleSetWizard from '@/components/ScoringRuleSetWizard.vue'
import MatchResultWizard from '@/components/MatchResultWizard.vue'
import MatchListAdmin from '@/components/MatchListAdmin.vue'
import MatchImportPanel from '@/components/MatchImportPanel.vue'
import type { MatchImportPreviewDto } from '@/api/types'
import TournamentSelect from '@/components/TournamentSelect.vue'
import { useTournamentsStore } from '@/stores/tournaments'

const tournamentsStore = useTournamentsStore()
const tournaments = computed(() => tournamentsStore.tournaments)

type AdminSection =
  | 'heroes'
  | 'maps'
  | 'tournaments'
  | 'hero-pool'
  | 'map-pool'
  | 'scoring'
  | 'matches'

type MatchViewMode = 'list' | 'create' | 'edit' | 'import'

const currentSection = ref<AdminSection | null>(null)
const matchViewMode = ref<MatchViewMode>('list')
const selectedTournamentId = ref<number | null>(null)
const selectedMatchId = ref<number | null>(null)
// A scraped match handed from MatchImportPanel to MatchResultWizard. Cleared on
// every return to the list so a later manual "Record match" never starts from a
// stale import.
const importPrefill = ref<MatchImportPreviewDto | null>(null)

const sections = [
  { id: 'heroes' as const, label: 'Heroes', description: 'Manage hero identities' },
  { id: 'maps' as const, label: 'Maps', description: 'Manage game boards' },
  {
    id: 'tournaments' as const,
    label: 'Tournaments',
    description: 'Create and update tournaments',
  },
  {
    id: 'hero-pool' as const,
    label: 'Hero Pools & Pricing',
    description: 'Set hero costs per tournament',
  },
  { id: 'map-pool' as const, label: 'Map Pools', description: 'Set legal maps per tournament' },
  {
    id: 'scoring' as const,
    label: 'Scoring Rules',
    description: 'Manage scoring rule sets and coefficients',
  },
  {
    id: 'matches' as const,
    label: 'Match Results',
    description: 'Record, correct, or delete match results',
  },
]

function selectSection(id: AdminSection) {
  currentSection.value = id
  matchViewMode.value = 'list'
  selectedTournamentId.value = null
  selectedMatchId.value = null
}

function goBack() {
  currentSection.value = null
  matchViewMode.value = 'list'
  selectedTournamentId.value = null
  selectedMatchId.value = null
}

function startMatchCreation(tournamentId: number) {
  matchViewMode.value = 'create'
  selectedTournamentId.value = tournamentId
  selectedMatchId.value = null
  importPrefill.value = null
}

function startMatchEdit(tournamentId: number, matchId: number) {
  matchViewMode.value = 'edit'
  selectedTournamentId.value = tournamentId
  selectedMatchId.value = matchId
}

function startMatchImport(tournamentId: number) {
  matchViewMode.value = 'import'
  selectedTournamentId.value = tournamentId
  selectedMatchId.value = null
  importPrefill.value = null
}

/** The import panel resolved a match — open the ordinary wizard seeded with it. */
function reviewImportedMatch(preview: MatchImportPreviewDto) {
  importPrefill.value = preview
  matchViewMode.value = 'create'
}

function returnToMatchList() {
  matchViewMode.value = 'list'
  selectedMatchId.value = null
  importPrefill.value = null
}
</script>

<template>
  <div class="mx-auto max-w-6xl">
    <!-- Dashboard grid -->
    <div v-if="!currentSection">
      <header class="mb-12">
        <h1 class="headline text-3xl text-cyan">Admin Command Center</h1>
        <p class="mt-2 text-ink-dim">Manage tournament data, heroes, maps, and match results</p>
      </header>

      <div class="grid gap-6 sm:grid-cols-2 xl:grid-cols-3">
        <button
          v-for="section in sections"
          :key="section.id"
          class="panel group flex cursor-pointer items-center justify-between gap-4 p-6 text-left transition-colors hover:border-cyan hover:bg-surface-mid"
          @click="selectSection(section.id)"
        >
          <div class="flex-1">
            <h2 class="headline text-xl text-ink">{{ section.label }}</h2>
            <p class="mt-2 text-sm text-ink-dim">{{ section.description }}</p>
          </div>
          <div class="text-2xl text-cyan opacity-50 transition-opacity group-hover:opacity-100">→</div>
        </button>
      </div>
    </div>

    <!-- Selected section content -->
    <div v-else>
      <header class="mb-8">
        <button class="font-mono text-sm text-ink-dim transition-colors hover:text-cyan" @click="goBack">
          ← Back to Dashboard
        </button>
        <h1 class="headline mt-3 text-3xl text-cyan">
          {{ sections.find((s) => s.id === currentSection)?.label }}
        </h1>
        <div v-if="currentSection === 'matches' && matchViewMode !== 'list'" class="mt-2">
          <button
            class="font-mono text-sm text-ink-dim transition-colors hover:text-cyan"
            @click="returnToMatchList"
          >
            ← Back to Match List
          </button>
        </div>
      </header>

      <div class="panel p-4 md:p-8">
        <HeroManagementWizard v-if="currentSection === 'heroes'" />
        <MapManagementWizard v-else-if="currentSection === 'maps'" />
        <TournamentManagementWizard v-else-if="currentSection === 'tournaments'" />
        <HeroPoolWizard v-else-if="currentSection === 'hero-pool'" />
        <MapPoolWizard v-else-if="currentSection === 'map-pool'" />
        <ScoringRuleSetWizard v-else-if="currentSection === 'scoring'" />

        <!-- Match Results Section -->
        <div v-else-if="currentSection === 'matches'">
          <div v-if="matchViewMode === 'list'">
            <div class="mb-8">
              <h3 class="headline text-xl text-cyan">Select Tournament</h3>
              <p class="mt-2 mb-4 text-sm text-ink-dim">
                Choose a tournament to view or manage its match results
              </p>
              <TournamentSelect
                id="match-tournament-select"
                v-model="selectedTournamentId"
                :tournaments="tournaments"
                label="Tournament *"
              />
              <p
                v-if="tournaments.length === 0"
                class="mt-2 font-mono text-xs text-ink-dim italic"
              >
                No tournaments exist yet. Create one in the Tournaments section first.
              </p>
            </div>

            <div v-if="selectedTournamentId" class="mt-8">
              <MatchListAdmin
                :tournament-id="selectedTournamentId"
                @create="() => startMatchCreation(selectedTournamentId!)"
                @import="() => startMatchImport(selectedTournamentId!)"
                @edit="(matchId) => startMatchEdit(selectedTournamentId!, matchId)"
              />
            </div>
          </div>

          <MatchImportPanel
            v-else-if="matchViewMode === 'import' && selectedTournamentId"
            :tournament-id="selectedTournamentId"
            @review="reviewImportedMatch"
            @cancel="returnToMatchList"
          />

          <MatchResultWizard
            v-else-if="matchViewMode === 'create' && selectedTournamentId"
            :tournament-id="selectedTournamentId"
            :mode="'create'"
            :prefill="importPrefill"
            @success="returnToMatchList"
            @cancel="returnToMatchList"
          />

          <MatchResultWizard
            v-else-if="matchViewMode === 'edit' && selectedTournamentId && selectedMatchId"
            :tournament-id="selectedTournamentId"
            :match-id="selectedMatchId"
            :mode="'edit'"
            @success="returnToMatchList"
            @cancel="returnToMatchList"
          />
        </div>
      </div>
    </div>
  </div>
</template>
