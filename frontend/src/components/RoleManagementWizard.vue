<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { api } from '@/api/client'
import { useAsyncRequest } from '@/composables/useAsyncRequest'
import { useTournamentsStore } from '@/stores/tournaments'
import type { RoleWeight, RosterRole } from '@/api/types'
import DestructiveConfirmPanel from '@/components/DestructiveConfirmPanel.vue'
import ErrorBanner from '@/components/ErrorBanner.vue'
import TournamentSelect from '@/components/TournamentSelect.vue'

/**
 * A tournament's roster roles: each one a name, an optional per-roster cap, and
 * the per-game stats it rewards. A manager gives every hero on their roster one
 * of these, and the stats sheet an admin uploads with a match is priced by it.
 *
 * Stat names are free-form on purpose — they are whatever the tournament's stats
 * sheet has columns for — so, like a scoring metric, nothing here blocks a name
 * for being unfamiliar. Only its *shape* is checked, by the server.
 */
const tournamentsStore = useTournamentsStore()

const selectedTournamentId = ref<number | null>(null)
const roles = ref<RosterRole[]>([])
const { loading, error, violations, run } = useAsyncRequest()
const showForm = ref(false)
const editingRole = ref<RosterRole | null>(null)
const deletingRole = ref<RosterRole | null>(null)

const form = ref({
  name: '',
  maxPerRoster: null as number | null,
  weights: [] as RoleWeight[],
})

const tournaments = computed(() => tournamentsStore.tournaments)
const selectedTournament = computed(() =>
  selectedTournamentId.value === null ? null : tournamentsStore.byId(selectedTournamentId.value),
)

watch(selectedTournamentId, () => {
  cancelForm()
  deletingRole.value = null
  void loadRoles()
})

async function loadRoles() {
  const tournamentId = selectedTournamentId.value
  if (!tournamentId) {
    roles.value = []
    return
  }
  const result = await run(() => api.roles(tournamentId), 'Failed to load roles')
  if (selectedTournamentId.value !== tournamentId) return
  if (result.ok) roles.value = result.value
}

function startCreate() {
  editingRole.value = null
  form.value = { name: '', maxPerRoster: null, weights: [{ stat: '', coefficient: 1 }] }
  showForm.value = true
}

function startEdit(role: RosterRole) {
  editingRole.value = role
  form.value = {
    name: role.name,
    maxPerRoster: role.maxPerRoster ?? null,
    weights: role.weights.map((w) => ({ ...w })),
  }
  showForm.value = true
}

function cancelForm() {
  showForm.value = false
  editingRole.value = null
  form.value = { name: '', maxPerRoster: null, weights: [] }
}

function addWeight() {
  form.value.weights.push({ stat: '', coefficient: 1 })
}

function removeWeight(index: number) {
  form.value.weights.splice(index, 1)
}

async function saveRole() {
  violations.value = []
  const tournamentId = selectedTournamentId.value
  if (!tournamentId) {
    error.value = 'Please select a tournament'
    return
  }
  if (!form.value.name.trim()) {
    error.value = 'Role name is required'
    return
  }
  if (form.value.weights.some((w) => !w.stat.trim())) {
    error.value = 'Every weight needs a stat name'
    return
  }

  const editing = editingRole.value
  const body = {
    name: form.value.name.trim(),
    // An empty number input reads back as '' — send that as uncapped.
    maxPerRoster: form.value.maxPerRoster || null,
    // A new role goes last; an edited one keeps its place.
    sortOrder: editing ? editing.sortOrder : roles.value.length + 1,
    weights: form.value.weights,
  }
  const fallback = 'Failed to save role'
  const result = editing
    ? await run(() => api.admin.updateRole(tournamentId, editing.id, body), fallback)
    : await run(() => api.admin.createRole(tournamentId, body), fallback)
  if (!result.ok) return
  cancelForm()
  await loadRoles()
}

async function confirmDelete() {
  const tournamentId = selectedTournamentId.value
  const role = deletingRole.value
  if (!tournamentId || !role) return
  const result = await run(
    () => api.admin.deleteRole(tournamentId, role.id),
    'Failed to delete role',
  )
  deletingRole.value = null
  if (result.ok) await loadRoles()
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">Roster Roles</h2>
    </div>

    <ErrorBanner :message="error" :violations="violations" />

    <TournamentSelect v-model="selectedTournamentId" :tournaments="tournaments" />

    <p
      v-if="selectedTournament && !selectedTournament.rolesEnabled"
      class="border border-edge bg-surface-lowest p-3 font-mono text-xs text-ink-dim"
    >
      Roles are switched off for {{ selectedTournament.name }}. You can still define them here;
      managers see them, and the standings price them, once Roster Roles is switched on in the
      tournament's settings.
    </p>

    <div v-if="selectedTournamentId && !showForm" class="flex gap-3">
      <button class="btn-primary" @click="startCreate">+ Create Role</button>
    </div>

    <!-- Form -->
    <div v-if="showForm" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">
        {{ editingRole ? `Edit “${editingRole.name}”` : 'Create Role' }}
      </h3>

      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div class="flex flex-col gap-2">
          <label for="role-name" class="label-caps">Role Name *</label>
          <input
            id="role-name"
            v-model="form.name"
            type="text"
            class="field-input"
            placeholder="e.g., Attacker, Healer"
          />
        </div>
        <div class="flex flex-col gap-2">
          <label for="role-cap" class="label-caps">Max Per Roster</label>
          <input
            id="role-cap"
            v-model.number="form.maxPerRoster"
            type="number"
            min="1"
            class="field-input"
            placeholder="Unlimited"
          />
          <p class="font-mono text-[11px] text-ink-dim">
            Leave empty for no limit. Checked when a roster locks.
          </p>
        </div>
      </div>

      <div class="flex flex-col gap-2">
        <span class="label-caps">Rewarded Stats</span>
        <p class="font-mono text-[11px] text-ink-dim">
          Each stat is a column of the match stats sheet; a hero in this role earns its count times
          the weight, every game it plays. A negative weight only offsets the others: a game's bonus
          never drops below 0.
        </p>

        <div class="flex flex-col gap-3">
          <div
            v-for="(weight, index) in form.weights"
            :key="index"
            class="flex flex-wrap items-end gap-4 border border-edge bg-surface-lowest p-4"
          >
            <div class="flex min-w-40 flex-1 flex-col gap-2">
              <label :for="`role-stat-${index}`" class="label-caps">Stat</label>
              <input
                :id="`role-stat-${index}`"
                v-model="weight.stat"
                type="text"
                class="field-input-sm bg-surface-low"
                placeholder="ATTACKS"
              />
            </div>
            <div class="flex w-32 flex-col gap-2">
              <label :for="`role-weight-${index}`" class="label-caps">Weight</label>
              <input
                :id="`role-weight-${index}`"
                v-model.number="weight.coefficient"
                type="number"
                step="0.05"
                class="field-input-sm bg-surface-low"
              />
            </div>
            <button
              type="button"
              class="btn-ghost px-4 py-2 text-xs"
              @click="removeWeight(index)"
            >
              Remove
            </button>
          </div>
        </div>

        <button type="button" class="btn-ghost" @click="addWeight">+ Add Stat</button>
      </div>

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="cancelForm">Cancel</button>
        <button class="btn-primary" :disabled="loading" @click="saveRole">
          {{ loading ? 'Saving...' : 'Save Role' }}
        </button>
      </div>
    </div>

    <DestructiveConfirmPanel
      v-if="deletingRole"
      :title="`Delete “${deletingRole.name}”?`"
      confirm-label="Delete Role"
      busy-label="Deleting…"
      :busy="loading"
      @cancel="deletingRole = null"
      @confirm="confirmDelete"
    >
      A role a manager has already assigned cannot be deleted — its rounds were scored under it.
      Rename or retune it instead.
    </DestructiveConfirmPanel>

    <!-- Role list -->
    <div v-if="selectedTournamentId && !showForm" class="flex flex-col gap-3">
      <p v-if="roles.length === 0 && !loading" class="p-12 text-center text-ink-dim">
        This tournament has no roles yet.
      </p>

      <div v-for="role in roles" :key="role.id" class="panel flex flex-col gap-3 p-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex items-center gap-3">
            <span class="headline text-base text-ink">{{ role.name }}</span>
            <span
              v-if="role.maxPerRoster"
              class="border border-edge px-2 py-0.5 font-mono text-[10px] tracking-[0.1em] text-ink-dim uppercase"
            >
              Max {{ role.maxPerRoster }}
            </span>
          </div>
          <div class="flex gap-2">
            <button
              :id="`edit-role-${role.id}`"
              class="btn-ghost px-4 py-2 text-xs"
              :disabled="loading"
              @click="startEdit(role)"
            >
              Edit
            </button>
            <button
              :id="`delete-role-${role.id}`"
              class="border border-magenta px-4 py-2 font-mono text-xs text-magenta transition-opacity hover:opacity-85"
              :disabled="loading"
              @click="deletingRole = role"
            >
              Delete
            </button>
          </div>
        </div>
        <div class="flex flex-wrap gap-2">
          <span
            v-for="weight in role.weights"
            :key="weight.stat"
            class="border border-edge bg-surface-lowest px-2 py-1 font-mono text-xs text-ink-dim"
          >
            {{ weight.stat }} × {{ weight.coefficient }}
          </span>
          <span v-if="role.weights.length === 0" class="font-mono text-xs text-ink-dim">
            Rewards nothing yet.
          </span>
        </div>
      </div>
    </div>
  </div>
</template>
