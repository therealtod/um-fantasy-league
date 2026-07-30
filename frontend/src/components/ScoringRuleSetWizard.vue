<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { api } from '@/api/client'
import { useTournamentsStore } from '@/stores/tournaments'
import type { ScoringCoefficientRequest } from '@/api/types'

const tournamentsStore = useTournamentsStore()

const selectedTournamentId = ref<number | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const showForm = ref(false)

const form = ref({
  name: '',
  coefficients: [] as ScoringCoefficientRequest[],
  activate: false,
})

const tournaments = computed(() => tournamentsStore.tournaments)

// Common metrics for quick reference
const commonMetrics = [
  'APPEARANCE',
  'BAN',
  'WIN',
  'LOSS',
  'DRAW',
  'HEALTH_REMAINING',
  'HEALTH_DIFFERENTIAL',
  'SHUTOUT',
]

watch(selectedTournamentId, () => {
  resetForm()
})

function resetForm() {
  form.value = {
    name: '',
    coefficients: [],
    activate: false,
  }
  showForm.value = false
}

function startCreate() {
  resetForm()
  // Add one empty coefficient to start
  form.value.coefficients.push({ metric: '', coefficient: 1.0, sortOrder: 0 })
  showForm.value = true
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

async function saveRuleSet() {
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

  try {
    await api.admin.createScoringRuleSet(selectedTournamentId.value, {
      name: form.value.name,
      coefficients: form.value.coefficients,
      activate: form.value.activate,
    })
    resetForm()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to save scoring rule set'
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

    <p v-if="error" class="border border-magenta/50 bg-magenta/10 p-4 font-mono text-sm text-magenta">
      {{ error }}
    </p>

    <!-- Tournament selector -->
    <div class="flex flex-col gap-2">
      <label for="tournament-select" class="label-caps">Select Tournament *</label>
      <select
        id="tournament-select"
        v-model.number="selectedTournamentId"
        class="cursor-pointer border border-edge bg-surface-lowest px-3 py-2 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
      >
        <option :value="null">-- Choose a tournament --</option>
        <option v-for="tournament in tournaments" :key="tournament.id" :value="tournament.id">
          {{ tournament.name }}
        </option>
      </select>
    </div>

    <!-- Create button -->
    <div v-if="selectedTournamentId && !showForm" class="flex gap-3">
      <button class="btn-primary" @click="startCreate">+ Create Scoring Rule Set</button>
    </div>

    <!-- Form -->
    <div v-if="showForm" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">Create Scoring Rule Set</h3>

      <div class="flex flex-col gap-2">
        <label for="rule-set-name" class="label-caps">Rule Set Name *</label>
        <input
          id="rule-set-name"
          v-model="form.name"
          type="text"
          class="border border-edge bg-surface-lowest px-3 py-2 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
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
                  class="border border-edge bg-surface-low px-2 py-1.5 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
                  placeholder="METRIC_NAME"
                />
                <div class="flex flex-wrap gap-1">
                  <button
                    v-for="metric in commonMetrics"
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
                  class="border border-edge bg-surface-low px-2 py-1.5 font-mono text-sm text-ink focus:border-cyan focus:outline-none"
                />
              </div>

              <div class="flex flex-col gap-2">
                <label :for="`sort-${index}`" class="label-caps">Order</label>
                <input
                  :id="`sort-${index}`"
                  v-model.number="coef.sortOrder"
                  type="number"
                  disabled
                  class="border border-edge bg-surface-low px-2 py-1.5 font-mono text-sm text-ink focus:border-cyan focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
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

      <div class="flex flex-col gap-2">
        <label class="flex cursor-pointer items-center gap-2 font-mono text-sm text-ink">
          <input v-model="form.activate" type="checkbox" />
          <span>Activate this rule set immediately</span>
        </label>
      </div>

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="resetForm">Cancel</button>
        <button class="btn-primary" :disabled="loading" @click="saveRuleSet">
          {{ loading ? 'Saving...' : 'Save Rule Set' }}
        </button>
      </div>
    </div>
  </div>
</template>
