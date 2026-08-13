<script setup lang="ts">
import { ref, computed } from 'vue'
import { api } from '@/api/client'
import { useTournamentsStore } from '@/stores/tournaments'
import type {
  Tournament,
  CreateTournamentRequest,
  TournamentFormat,
  TournamentStatus,
} from '@/api/types'

const tournamentsStore = useTournamentsStore()

const loading = ref(false)
const error = ref<string | null>(null)
const showForm = ref(false)
const editingTournament = ref<Tournament | null>(null)
const deletingTournamentId = ref<number | null>(null)
const deletingTournamentName = ref<string>('')
const showDeleteConfirm = ref(false)

const form = ref<CreateTournamentRequest>({
  name: '',
  format: 'BANQUEST',
  status: 'SCHEDULED',
  startDate: '',
  endDate: null,
  capacity: 64,
  rosterSize: 3,
  creditGrant: 10000,
})

const formatOptions: { value: TournamentFormat; label: string }[] = [
  { value: 'BANQUEST', label: 'Banquest' },
  { value: 'ARSENAL', label: 'Arsenal' },
]

const statusOptions: { value: TournamentStatus; label: string }[] = [
  { value: 'SCHEDULED', label: 'Scheduled' },
  { value: 'REGISTRATION_OPEN', label: 'Registration Open' },
  { value: 'LIVE', label: 'Live' },
  { value: 'COMPLETED', label: 'Completed' },
]

const tournaments = computed(() => tournamentsStore.tournaments)

function startCreate() {
  editingTournament.value = null
  form.value = {
    name: '',
    format: 'BANQUEST',
    status: 'SCHEDULED',
    startDate: '',
    endDate: null,
    capacity: 64,
    rosterSize: 3,
    creditGrant: 10000,
  }
  showForm.value = true
}

function startEdit(tournament: Tournament) {
  editingTournament.value = tournament
  form.value = {
    name: tournament.name,
    format: tournament.format,
    status: tournament.status,
    startDate: tournament.startDate,
    endDate: tournament.endDate ?? null,
    capacity: tournament.capacity,
    rosterSize: tournament.rosterSize,
    creditGrant: tournament.creditGrant,
  }
  showForm.value = true
}

function cancelForm() {
  showForm.value = false
  editingTournament.value = null
}

function startDelete(tournament: Tournament) {
  deletingTournamentId.value = tournament.id
  deletingTournamentName.value = tournament.name
  showDeleteConfirm.value = true
}

function cancelDelete() {
  showDeleteConfirm.value = false
  deletingTournamentId.value = null
  deletingTournamentName.value = ''
}

async function confirmDelete() {
  if (deletingTournamentId.value === null) return

  loading.value = true
  error.value = null

  try {
    await api.admin.deleteTournament(deletingTournamentId.value)
    await tournamentsStore.load() // Refresh the list
    cancelDelete()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to delete tournament'
  } finally {
    loading.value = false
  }
}

async function saveTournament() {
  if (!form.value.name.trim()) {
    error.value = 'Tournament name is required'
    return
  }
  if (!form.value.startDate) {
    error.value = 'Start date is required'
    return
  }

  loading.value = true
  error.value = null

  try {
    if (editingTournament.value) {
      await api.admin.updateTournament(editingTournament.value.id, form.value)
    } else {
      await api.admin.createTournament(form.value)
    }
    await tournamentsStore.load() // Refresh the list
    cancelForm()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to save tournament'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">Tournament Management</h2>
      <button v-if="!showForm" class="btn-primary" @click="startCreate">
        + Create Tournament
      </button>
    </div>

    <p v-if="error" class="border border-magenta/50 bg-magenta/10 p-4 font-mono text-sm text-magenta">
      {{ error }}
    </p>

    <!-- Form -->
    <div v-if="showForm" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">
        {{ editingTournament ? 'Edit Tournament' : 'Create New Tournament' }}
      </h3>

      <div class="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
        <div class="flex flex-col gap-2 sm:col-span-2 lg:col-span-3">
          <label for="tournament-name" class="label-caps">Name *</label>
          <input
            id="tournament-name"
            v-model="form.name"
            type="text"
            class="field-input"
            placeholder="e.g., Summer of Legends"
          />
        </div>

        <div class="flex flex-col gap-2">
          <label for="tournament-format" class="label-caps">Format *</label>
          <select
            id="tournament-format"
            v-model="form.format"
            class="cursor-pointer field-input"
          >
            <option v-for="opt in formatOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </option>
          </select>
        </div>

        <div class="flex flex-col gap-2">
          <label for="tournament-status" class="label-caps">Status *</label>
          <select
            id="tournament-status"
            v-model="form.status"
            class="cursor-pointer field-input"
          >
            <option v-for="opt in statusOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </option>
          </select>
        </div>

        <div class="flex flex-col gap-2">
          <label for="tournament-start" class="label-caps">Start Date *</label>
          <input
            id="tournament-start"
            v-model="form.startDate"
            type="date"
            class="field-input"
          />
        </div>

        <div class="flex flex-col gap-2">
          <label for="tournament-end" class="label-caps">End Date (optional)</label>
          <input
            id="tournament-end"
            v-model="form.endDate"
            type="date"
            class="field-input"
          />
        </div>

        <div class="flex flex-col gap-2">
          <label for="tournament-capacity" class="label-caps">Capacity *</label>
          <input
            id="tournament-capacity"
            v-model.number="form.capacity"
            type="number"
            min="1"
            class="field-input"
          />
        </div>

        <div class="flex flex-col gap-2">
          <label for="tournament-roster-size" class="label-caps">Roster Size *</label>
          <input
            id="tournament-roster-size"
            v-model.number="form.rosterSize"
            type="number"
            min="1"
            class="field-input"
          />
        </div>

        <div class="flex flex-col gap-2">
          <label for="tournament-credit" class="label-caps">Credit Grant *</label>
          <input
            id="tournament-credit"
            v-model.number="form.creditGrant"
            type="number"
            min="1"
            class="field-input"
            placeholder="e.g., 10000"
          />
        </div>
      </div>

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="cancelForm">Cancel</button>
        <button class="btn-primary" :disabled="loading" @click="saveTournament">
          {{ loading ? 'Saving...' : 'Save Tournament' }}
        </button>
      </div>
    </div>

    <!-- Delete confirmation -->
    <div v-if="showDeleteConfirm" class="panel flex flex-col gap-5 border-magenta p-6">
      <h3 class="headline text-lg text-magenta">Delete Tournament</h3>
      <p class="font-mono text-sm leading-relaxed text-ink-dim">
        Are you sure you want to delete <strong>{{ deletingTournamentName }}</strong>? This
        permanently removes its hero pool, map pool, scoring rules, manager entries, and recorded
        matches. This cannot be undone.
      </p>
      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="cancelDelete">Cancel</button>
        <button
          class="border border-magenta px-6 py-3 font-mono text-sm text-magenta transition-opacity hover:opacity-85 disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="loading"
          @click="confirmDelete"
        >
          {{ loading ? 'Deleting...' : 'Delete Tournament' }}
        </button>
      </div>
    </div>

    <!-- Tournaments list -->
    <div v-if="!showForm && !showDeleteConfirm" class="flex flex-col gap-2">
      <div v-if="tournaments.length === 0" class="p-12 text-center">
        <p class="text-ink-dim">No tournaments found. Create a new tournament to begin.</p>
      </div>

      <div
        v-for="tournament in tournaments"
        :key="tournament.id"
        class="panel flex flex-wrap items-center justify-between gap-3 p-4"
      >
        <div class="flex flex-col gap-2">
          <span class="headline text-base text-ink">{{ tournament.name }}</span>
          <div class="flex flex-wrap gap-4">
            <span class="font-mono text-xs text-ink-dim">{{ tournament.format }}</span>
            <span class="font-mono text-xs text-ink-dim">{{ tournament.status }}</span>
            <span class="font-mono text-xs text-ink-dim">{{ tournament.startDate }}</span>
            <span class="font-mono text-xs text-ink-dim">
              {{ tournament.enrolled }}/{{ tournament.capacity }}
            </span>
          </div>
        </div>
        <div class="flex gap-2">
          <button class="btn-ghost px-4 py-2 text-xs" @click="startEdit(tournament)">Edit</button>
          <button
            class="border border-magenta px-4 py-2 font-mono text-xs text-magenta transition-opacity hover:opacity-85"
            @click="startDelete(tournament)"
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
