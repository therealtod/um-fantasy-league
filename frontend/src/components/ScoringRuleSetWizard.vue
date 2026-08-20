<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { api, describeError, violationMessages } from '@/api/client'
import { useTournamentsStore } from '@/stores/tournaments'
import type { ScoringCoefficientRequest, ScoringRuleSetDto } from '@/api/types'
import ErrorBanner from '@/components/ErrorBanner.vue'
import TournamentSelect from '@/components/TournamentSelect.vue'

const tournamentsStore = useTournamentsStore()

const selectedTournamentId = ref<number | null>(null)
const ruleSets = ref<ScoringRuleSetDto[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const violations = ref<string[]>([])
const showForm = ref(false)
const editingRuleSet = ref<ScoringRuleSetDto | null>(null)

/**
 * The `warnings` off the last save. An unknown metric is accepted on purpose
 * (`MatchMetrics` scores it zero and drops it from the leaderboard's columns),
 * so a clean 201 is the only signal an admin would otherwise get for a typo.
 */
const savedWarnings = ref<{ name: string; metrics: string[] } | null>(null)

const form = ref({
  name: '',
  coefficients: [] as ScoringCoefficientRequest[],
  activate: false,
})

const tournaments = computed(() => tournamentsStore.tournaments)

/**
 * Mirrors `MatchMetrics`' extractor registry. Purely a hint: the backend takes
 * any metric string and reports the unpriced ones as warnings, so nothing here
 * blocks a save — see `unknownFormMetrics`.
 */
const knownMetrics = [
  'APPEARANCE',
  'SELF_BAN',
  'OPPONENT_BAN',
  'WIN',
  'LOSS',
  'HEALTH_REMAINING',
  'HEALTH_DIFFERENTIAL',
  'HEALTH_DIFFERENTIAL_TWO_WAY',
  'SHUTOUT',
]

/** Same normalisation as `MatchMetrics.normalise`, so the hint matches the server's verdict. */
function normaliseMetric(metric: string): string {
  return metric.trim().toUpperCase()
}

function isUnknownMetric(metric: string): boolean {
  const normalised = normaliseMetric(metric)
  return normalised.length > 0 && !knownMetrics.includes(normalised)
}

/** Distinct unpriced metrics in the open form — warned about before the round trip, never rejected. */
const unknownFormMetrics = computed(() => [
  ...new Set(
    form.value.coefficients
      .map((c) => normaliseMetric(c.metric))
      .filter((m) => m.length > 0 && !knownMetrics.includes(m)),
  ),
])

watch(selectedTournamentId, () => {
  cancelForm()
  savedWarnings.value = null
  void loadRuleSets()
})

async function loadRuleSets() {
  const tournamentId = selectedTournamentId.value
  if (!tournamentId) {
    ruleSets.value = []
    return
  }

  loading.value = true
  error.value = null
  violations.value = []
  try {
    const loaded = await api.admin.listScoringRuleSets(tournamentId)
    // A later switch may have already changed the selection while this was in flight — drop it.
    if (selectedTournamentId.value !== tournamentId) return
    ruleSets.value = loaded
  } catch (e) {
    if (selectedTournamentId.value !== tournamentId) return
    error.value = describeError(e, 'Failed to load scoring rule sets')
    violations.value = violationMessages(e)
  } finally {
    if (selectedTournamentId.value === tournamentId) loading.value = false
  }
}

function startCreate() {
  editingRuleSet.value = null
  savedWarnings.value = null
  form.value = {
    name: '',
    // One empty coefficient to start — the backend requires at least one.
    coefficients: [{ metric: '', coefficient: 1.0, sortOrder: 0 }],
    activate: false,
  }
  showForm.value = true
}

function startEdit(ruleSet: ScoringRuleSetDto) {
  editingRuleSet.value = ruleSet
  savedWarnings.value = null
  form.value = {
    name: ruleSet.name,
    coefficients: ruleSet.coefficients.map((c, i) => ({
      metric: c.metric,
      coefficient: c.coefficient,
      sortOrder: i,
    })),
    activate: false,
  }
  showForm.value = true
}

function cancelForm() {
  showForm.value = false
  editingRuleSet.value = null
  form.value = { name: '', coefficients: [], activate: false }
}

function addCoefficient() {
  form.value.coefficients.push({
    metric: '',
    coefficient: 1.0,
    sortOrder: form.value.coefficients.length,
  })
}

function removeCoefficient(index: number) {
  form.value.coefficients.splice(index, 1)
  // Renumber sortOrder
  form.value.coefficients.forEach((c, i) => {
    c.sortOrder = i
  })
}

function useMetric(index: number, metric: string) {
  form.value.coefficients[index].metric = metric
}

/** Replaces one rule set in the list, or appends it if it's new. */
function mergeRuleSet(saved: ScoringRuleSetDto) {
  const index = ruleSets.value.findIndex((r) => r.id === saved.id)
  if (index === -1) {
    ruleSets.value.push(saved)
  } else {
    ruleSets.value[index] = saved
  }
}

async function saveRuleSet() {
  violations.value = []

  if (!selectedTournamentId.value) {
    error.value = 'Please select a tournament'
    return
  }
  if (!form.value.name.trim()) {
    error.value = 'Rule set name is required'
    return
  }
  if (form.value.coefficients.length === 0) {
    error.value = 'At least one coefficient is required'
    return
  }

  // Validate all coefficients have metrics
  const emptyMetrics = form.value.coefficients.filter((c) => !c.metric.trim())
  if (emptyMetrics.length > 0) {
    error.value = 'All coefficients must have a metric'
    return
  }

  loading.value = true
  error.value = null
  violations.value = []
  savedWarnings.value = null

  try {
    const editing = editingRuleSet.value
    const saved = editing
      ? await api.admin.updateScoringRuleSet(selectedTournamentId.value, editing.id, {
          name: form.value.name,
          coefficients: form.value.coefficients,
        })
      : await api.admin.createScoringRuleSet(selectedTournamentId.value, {
          name: form.value.name,
          coefficients: form.value.coefficients,
          activate: form.value.activate,
        })

    savedWarnings.value = { name: saved.name, metrics: saved.warnings ?? [] }
    cancelForm()
    // Activating deactivates a sibling, so the whole list is stale after a create+activate.
    if (saved.isActive) {
      await loadRuleSets()
    } else {
      mergeRuleSet(saved)
    }
  } catch (e) {
    error.value = describeError(e, 'Failed to save scoring rule set')
    violations.value = violationMessages(e)
  } finally {
    loading.value = false
  }
}

async function activateRuleSet(ruleSet: ScoringRuleSetDto) {
  if (!selectedTournamentId.value) return

  loading.value = true
  error.value = null
  violations.value = []
  try {
    await api.admin.activateScoringRuleSet(selectedTournamentId.value, ruleSet.id)
    // Reload rather than patch: the previously active sibling was deactivated too.
    await loadRuleSets()
  } catch (e) {
    error.value = describeError(e, 'Failed to activate scoring rule set')
    violations.value = violationMessages(e)
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">Scoring Rule Sets</h2>
    </div>

    <ErrorBanner :message="error" :violations="violations" />

    <!-- Post-save warnings: the save succeeded, but nothing scores these metrics. -->
    <div
      v-if="savedWarnings"
      class="flex flex-col gap-2 border p-4 font-mono text-sm"
      :class="
        savedWarnings.metrics.length === 0
          ? 'border-lime/50 bg-lime/10 text-lime'
          : 'border-danger/50 bg-danger/10 text-danger'
      "
    >
      <p v-if="savedWarnings.metrics.length === 0">
        Saved “{{ savedWarnings.name }}”. Every metric is priced by the scoring engine.
      </p>
      <template v-else>
        <p class="font-bold">
          Saved “{{ savedWarnings.name }}”, but {{ savedWarnings.metrics.length }} metric{{
            savedWarnings.metrics.length === 1 ? '' : 's'
          }}
          will score zero:
        </p>
        <ul class="flex flex-wrap gap-2">
          <li v-for="metric in savedWarnings.metrics" :key="metric" class="border border-danger/50 px-2 py-1">
            {{ metric }}
          </li>
        </ul>
        <p class="text-xs">
          No extractor implements these, so they add no points and never appear as a leaderboard
          column. Check for a typo, or leave them if they are placeholders.
        </p>
      </template>
    </div>

    <!-- Tournament selector -->
    <TournamentSelect v-model="selectedTournamentId" :tournaments="tournaments" />

    <!-- Create button -->
    <div v-if="selectedTournamentId && !showForm" class="flex gap-3">
      <button class="btn-primary" @click="startCreate">+ Create Scoring Rule Set</button>
    </div>

    <!-- Form -->
    <div v-if="showForm" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">
        {{ editingRuleSet ? `Edit “${editingRuleSet.name}”` : 'Create Scoring Rule Set' }}
      </h3>

      <div class="flex flex-col gap-2">
        <label for="rule-set-name" class="label-caps">Rule Set Name *</label>
        <input
          id="rule-set-name"
          v-model="form.name"
          type="text"
          class="field-input"
          placeholder="e.g., Standard Scoring, Advanced Metrics"
        />
      </div>

      <div class="flex flex-col gap-2">
        <span class="label-caps">Coefficients *</span>

        <div class="flex flex-col gap-4">
          <div
            v-for="(coef, index) in form.coefficients"
            :key="index"
            class="flex flex-wrap items-start gap-4 border border-edge bg-surface-lowest p-4"
          >
            <div class="grid flex-1 grid-cols-2 gap-4 sm:grid-cols-3">
              <div class="flex flex-col gap-2">
                <label :for="`metric-${index}`" class="label-caps">Metric</label>
                <input
                  :id="`metric-${index}`"
                  v-model="coef.metric"
                  type="text"
                  class="field-input-sm bg-surface-low"
                  :class="isUnknownMetric(coef.metric) ? 'border-danger text-danger' : ''"
                  placeholder="METRIC_NAME"
                />
                <p v-if="isUnknownMetric(coef.metric)" class="font-mono text-[10px] text-danger">
                  Nothing scores this metric — it will be worth zero points.
                </p>
                <div class="flex flex-wrap gap-1">
                  <button
                    v-for="metric in knownMetrics"
                    :key="metric"
                    type="button"
                    class="border border-edge bg-surface-low px-2 py-1 font-mono text-[10px] text-ink-dim transition-colors hover:border-cyan hover:text-cyan"
                    @click="useMetric(index, metric)"
                  >
                    {{ metric }}
                  </button>
                </div>
              </div>

              <div class="flex flex-col gap-2">
                <label :for="`coefficient-${index}`" class="label-caps">Coefficient</label>
                <input
                  :id="`coefficient-${index}`"
                  v-model.number="coef.coefficient"
                  type="number"
                  step="0.1"
                  class="field-input-sm bg-surface-low"
                />
              </div>

              <div class="flex flex-col gap-2">
                <label :for="`sort-${index}`" class="label-caps">Order</label>
                <input
                  :id="`sort-${index}`"
                  v-model.number="coef.sortOrder"
                  type="number"
                  disabled
                  class="field-input-sm bg-surface-low disabled:cursor-not-allowed disabled:opacity-50"
                />
              </div>
            </div>

            <button
              type="button"
              class="btn-ghost px-4 py-2 text-xs"
              @click="removeCoefficient(index)"
            >
              Remove
            </button>
          </div>
        </div>

        <button type="button" class="btn-ghost" @click="addCoefficient">
          + Add Coefficient
        </button>
      </div>

      <p
        v-if="unknownFormMetrics.length > 0"
        class="border border-danger/50 bg-danger/10 p-3 font-mono text-xs text-danger"
      >
        {{ unknownFormMetrics.join(', ') }} — no extractor implements
        {{ unknownFormMetrics.length === 1 ? 'this metric' : 'these metrics' }}. Saving is allowed;
        {{ unknownFormMetrics.length === 1 ? 'it' : 'they' }} will simply score zero.
      </p>

      <div v-if="!editingRuleSet" class="flex flex-col gap-2">
        <label class="flex cursor-pointer items-center gap-2 font-mono text-sm text-ink">
          <input v-model="form.activate" type="checkbox" />
          <span>Activate this rule set immediately</span>
        </label>
      </div>

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="cancelForm">Cancel</button>
        <button class="btn-primary" :disabled="loading" @click="saveRuleSet">
          {{ loading ? 'Saving...' : 'Save Rule Set' }}
        </button>
      </div>
    </div>

    <!-- Rule set list -->
    <div v-if="selectedTournamentId && !showForm" class="flex flex-col gap-3">
      <p v-if="ruleSets.length === 0 && !loading" class="p-12 text-center text-ink-dim">
        This tournament has no scoring rule sets yet.
      </p>

      <div v-for="ruleSet in ruleSets" :key="ruleSet.id" class="panel flex flex-col gap-4 p-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex items-center gap-3">
            <span class="headline text-base text-ink">{{ ruleSet.name }}</span>
            <span
              v-if="ruleSet.isActive"
              class="border border-lime/50 bg-lime/10 px-2 py-0.5 font-mono text-[10px] tracking-[0.1em] text-lime uppercase"
            >
              Active
            </span>
          </div>
          <div class="flex gap-2">
            <button
              v-if="!ruleSet.isActive"
              :id="`activate-${ruleSet.id}`"
              class="border border-lime px-3 py-1 font-mono text-xs text-lime transition-colors hover:bg-lime/10"
              :disabled="loading"
              @click="activateRuleSet(ruleSet)"
            >
              Activate
            </button>
            <button
              :id="`edit-${ruleSet.id}`"
              class="btn-ghost px-4 py-2 text-xs"
              :disabled="loading"
              @click="startEdit(ruleSet)"
            >
              Edit
            </button>
          </div>
        </div>

        <div class="flex flex-wrap gap-2">
          <span
            v-for="coef in ruleSet.coefficients"
            :key="coef.metric"
            class="border border-edge bg-surface-lowest px-2 py-1 font-mono text-xs"
            :class="ruleSet.warnings?.includes(coef.metric) ? 'text-danger' : 'text-ink-dim'"
          >
            {{ coef.metric }} × {{ coef.coefficient }}
          </span>
        </div>

        <p v-if="ruleSet.warnings?.length" class="font-mono text-xs text-danger">
          Unscored: {{ ruleSet.warnings.join(', ') }} — worth zero points, and absent from the
          leaderboard's columns.
        </p>
      </div>
    </div>
  </div>
</template>
