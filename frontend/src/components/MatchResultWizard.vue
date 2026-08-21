<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { api, describeError, violationMessages } from '@/api/client'
import { useAsyncRequest } from '@/composables/useAsyncRequest'
import { useTournamentsStore } from '@/stores/tournaments'
import { byName } from '@/lib/sort'
import type { Hero, MapAdminDto, MatchImportPreviewDto } from '@/api/types'
// The form model, the seeding, the payload conversion, the option lists and the
// validation all live in the domain module, as plain functions over plain data.
// What is left here is the reactive wrapper, the API calls, and the shell the
// section components hang off — so a `matchForm.` call site is the marker for
// "this is a rule, tested in matchForm.spec.ts", and anything without one is
// rendering.
import * as matchForm from '@/domain/matchForm'
import type { MatchForm } from '@/domain/matchForm'
import ErrorBanner from '@/components/ErrorBanner.vue'
// One section per part of a series, each taking the whole form as its model:
// the option lists are form-wide rules (a pre-ban has to know both drafts), so
// a section that only saw its own slice could not ask them anything.
import MatchUnassignedBanSection from '@/components/MatchUnassignedBanSection.vue'
import MatchDraftSide from '@/components/MatchDraftSide.vue'
import MatchGameRow from '@/components/MatchGameRow.vue'
import MatchPreBanSection from '@/components/MatchPreBanSection.vue'

interface Props {
  tournamentId?: number
  matchId?: number
  mode: 'create' | 'edit'
  /**
   * A scraped match to seed the form with, from the import panel. Create mode
   * only — an import produces a new match, never an edit of an existing one.
   */
  prefill?: MatchImportPreviewDto | null
}

const props = defineProps<Props>()
const emit = defineEmits<{
  success: []
  cancel: []
}>()

const tournamentsStore = useTournamentsStore()

// What the *server* (or a failed load) said, as opposed to what this form can
// work out for itself — see `validationError`.
const { loading, error: serverError, violations, run } = useAsyncRequest()
const submitAttempted = ref(false)
const isInitialized = ref(false)

// The tournament's legal boards and priced hero pool — what the match actually
// scores against — so the admin picks from a list instead of typing a raw id.
const mapPool = ref<MapAdminDto[]>([])
const heroPool = ref<Hero[]>([])

const form = ref<MatchForm>(matchForm.blankForm())

const tournaments = computed(() => tournamentsStore.tournaments)

// form.playedAt is always a full ISO instant (what the server sends and expects),
// but <input type="datetime-local"> only accepts/emits "YYYY-MM-DDTHH:mm" — anything
// else is silently sanitized to "" by the browser. This bridges the two formats
// instead of binding the input straight to form.playedAt.
const playedAtLocal = computed({
  get() {
    const iso = form.value.playedAt
    if (!iso) return ''
    const date = new Date(iso)
    if (Number.isNaN(date.getTime())) return ''
    const pad = (n: number) => String(n).padStart(2, '0')
    return (
      `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
      `T${pad(date.getHours())}:${pad(date.getMinutes())}`
    )
  },
  set(local: string) {
    if (!local) return
    const date = new Date(local)
    if (Number.isNaN(date.getTime())) return
    form.value.playedAt = date.toISOString()
  },
})

function resetForm() {
  form.value = props.prefill ? matchForm.formFromPreview(props.prefill) : matchForm.blankForm()
}

async function loadMatchData() {
  if (props.mode !== 'edit' || !props.tournamentId || !props.matchId) return
  const tournamentId = props.tournamentId
  const matchId = props.matchId

  const result = await run(() => api.admin.getMatch(tournamentId, matchId), 'Failed to load match data')
  if (result.ok) form.value = matchForm.formFromMatch(result.value)
}

async function loadPools() {
  const tournamentId = props.tournamentId
  if (!tournamentId) return

  const [maps, heroes] = await Promise.all([
    api.admin.listMapPool(tournamentId),
    api.admin.listHeroPool(tournamentId),
  ])
  mapPool.value = maps.sort(byName)
  heroPool.value = heroes.sort(byName)
}

// Appending a game is the one edit left at this level, since it is the only one
// no single section owns. Everything else — a removal that renumbers, a draft
// pick that cascades, a winner that moves sides — belongs to the section that
// renders it, and through it to the domain module.
function addGame() {
  form.value.games.push(matchForm.blankGame(form.value.games.length + 1))
}

/**
 * What this form can rule out on its own, live.
 *
 * A computed rather than a step inside `saveMatch` on purpose: an admin who
 * fixes the problem in place sees the banner clear as they type, instead of a
 * stale complaint that survives until the next save.
 */
const validationError = computed(() => matchForm.validate(form.value, heroPool.value))

/** The server's complaint if there is one, otherwise this form's own. */
const error = computed(() => serverError.value ?? (submitAttempted.value ? validationError.value : null))

// A rejected save describes a payload that no longer exists the moment the
// admin edits the form, so the banner clears on the correction rather than
// standing until the next attempt. `validationError` needs no equivalent: it is
// computed off the form and re-evaluates itself.
watch(
  form,
  () => {
    if (!isInitialized.value) return
    serverError.value = null
    violations.value = []
  },
  { deep: true },
)

async function saveMatch() {
  submitAttempted.value = true
  serverError.value = null
  violations.value = []

  const tournamentId = props.tournamentId
  if (!tournamentId) {
    serverError.value = 'Tournament ID is required'
    return
  }
  if (validationError.value) return

  const result = await run(async () => {
    if (props.mode === 'create') {
      await api.admin.recordMatch(tournamentId, matchForm.toPayload(form.value))
    } else if (props.mode === 'edit' && props.matchId) {
      await api.admin.correctMatch(tournamentId, props.matchId, matchForm.toPayload(form.value))
    }
  }, 'Failed to save match')

  if (result.ok) emit('success')
}

function handleCancel() {
  emit('cancel')
}

// Initialize based on mode. Manual rather than run(), since loadMatchData()
// already wraps its own call in run() on this same instance — nesting a
// second run() around it here would fight over one token/loading pair for
// no benefit; this just shares the same error/violations/loading refs.
onMounted(async () => {
  loading.value = true
  try {
    await loadPools()
    if (props.mode === 'edit') {
      await loadMatchData()
    } else {
      resetForm()
    }
  } catch (e) {
    serverError.value = describeError(e, 'Failed to load map and hero pools')
    violations.value = violationMessages(e)
  } finally {
    loading.value = false
  }
  isInitialized.value = true
})
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">
        {{ mode === 'create' ? 'Record Match Result' : 'Edit Match Result' }}
      </h2>
    </div>

    <ErrorBanner :message="error" :violations="violations" />

    <!-- Tournament info (when provided as prop) -->
    <div v-if="tournamentId" class="panel flex items-center gap-4 p-4">
      <div class="label-caps">Tournament</div>
      <div class="headline text-base text-cyan">
        {{ tournaments.find(t => t.id === tournamentId)?.name || `ID: ${tournamentId}` }}
      </div>
    </div>

    <!-- Loading state -->
    <div v-if="loading && !isInitialized" class="p-12 text-center font-mono text-ink-dim">
      Loading...
    </div>

    <!-- Form -->
    <div v-else class="panel flex flex-col gap-6 p-6">
      <h3 class="headline text-lg text-cyan">{{ mode === 'create' ? 'Record New Match' : 'Edit Match' }}</h3>

      <div class="grid gap-4 sm:grid-cols-2">
        <div class="flex flex-col gap-2">
          <label for="match-round" class="label-caps">Round *</label>
          <input
            id="match-round"
            v-model.number="form.round"
            type="number"
            min="1"
            class="field-input"
          />
        </div>

        <div class="flex flex-col gap-2">
          <label for="match-played-at" class="label-caps">Played At (ISO timestamp) *</label>
          <input
            id="match-played-at"
            v-model="playedAtLocal"
            type="datetime-local"
            class="field-input"
          />
        </div>

        <div class="flex flex-col gap-2 sm:col-span-2">
          <label for="match-external-link" class="label-caps">External Link</label>
          <input
            id="match-external-link"
            v-model="form.externalLink"
            type="text"
            placeholder="https://…"
            class="field-input"
          />
          <p class="text-xs text-ink-dim">
            Where this match is recorded elsewhere — the import source, a bracket page, a VOD. It
            has to be unique within the tournament: it is what stops the same match being recorded
            twice. For a match with no page anywhere, any identifier of your own will do.
          </p>
        </div>
      </div>

      <!-- Bans that predate per-side attribution. Nothing else can be filled in
           sensibly until these are placed, so they sit above the drafts. -->
      <MatchUnassignedBanSection
        v-if="form.unassignedBans.length"
        v-model="form"
        :hero-pool="heroPool"
      />

      <!-- The draft comes first, and everything below it is filtered to what it
           says. A side's list is its whole arsenal: the heroes it fielded, the
           ones it benched, and the ones it lost to a ban. -->
      <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
        <h4 class="headline text-base text-cyan">Drafts (exactly 2 sides, for the whole series)</h4>
        <p class="font-mono text-xs leading-relaxed text-ink-dim">
          Enter every hero each side took, whether or not it reached the table. The games and bans
          below only offer that side's own heroes.
        </p>

        <MatchDraftSide
          v-for="(_side, index) in form.sides"
          :key="index"
          v-model="form"
          :hero-pool="heroPool"
          :side="index"
        />
      </div>

      <!-- Games -->
      <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
        <h4 class="headline text-base text-cyan">Games (best-of-N — at least 1 required)</h4>

        <MatchGameRow
          v-for="(_game, gameIndex) in form.games"
          :key="gameIndex"
          v-model="form"
          :hero-pool="heroPool"
          :map-pool="mapPool"
          :index="gameIndex"
        />

        <button type="button" class="btn-ghost" @click="addGame">+ Add Game</button>
      </div>

      <!-- Pre-bans: struck before sides were assigned, so they belong to neither
           draft and score neither ban metric. -->
      <MatchPreBanSection v-model="form" :hero-pool="heroPool" />

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="handleCancel">Cancel</button>
        <button class="btn-primary" :disabled="loading" @click="saveMatch">
          {{ loading ? (mode === 'create' ? 'Recording...' : 'Updating...') : (mode === 'create' ? 'Record Match' : 'Update Match') }}
        </button>
      </div>

      <div v-if="!loading && (mapPool.length === 0 || heroPool.length === 0)" class="border border-edge bg-surface-lowest p-4">
        <p class="mb-2 font-mono text-xs text-cyan uppercase">Nothing to pick from yet</p>
        <p class="font-mono text-xs leading-relaxed text-ink-dim">
          <span v-if="mapPool.length === 0">This tournament's map pool is empty — add one in the Maps section. </span>
          <span v-if="heroPool.length === 0">This tournament's hero pool is empty — price a hero in first. </span>
          Player names are free text: type anything, or leave it blank. Points are scored per hero,
          so the name is only ever shown, never counted.
        </p>
      </div>
    </div>
  </div>
</template>
