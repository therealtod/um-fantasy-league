<script setup lang="ts">
import { ref, computed } from 'vue'
import { api, describeError, violationMessages } from '@/api/client'
import type { MatchImportPreviewDto, UnresolvedName } from '@/api/types'
import ErrorBanner from '@/components/ErrorBanner.vue'

/**
 * Imports one match from a Tabletop League URL.
 *
 * Deliberately a review step rather than a one-click record: the source page
 * carries no round number this schema can use, and a name the catalogue doesn't
 * know has to be an admin's decision, not a guess. So this panel scrapes,
 * reports what resolved and what didn't, and hands a filled-in form to
 * `MatchResultWizard` — which then saves through the ordinary record endpoint,
 * so an imported match is validated exactly like a typed one.
 */
interface Props {
  tournamentId: number
}
const props = defineProps<Props>()
const emit = defineEmits<{
  /** The admin accepted the draft — the parent opens the wizard seeded with it. */
  review: [preview: MatchImportPreviewDto]
  cancel: []
}>()

const sourceUrl = ref('')
const preview = ref<MatchImportPreviewDto | null>(null)
const loading = ref(false)
const addingMapId = ref<number | null>(null)
const error = ref<string | null>(null)
const violations = ref<string[]>([])

const canImport = computed(() => sourceUrl.value.trim().length > 0 && !loading.value)
const isResolved = computed(() => preview.value !== null && preview.value.unresolved.length === 0)

/**
 * Boards that exist but aren't in this tournament's pool. These are the only
 * unresolved rows this panel can fix in place — an unknown hero or board needs
 * a catalogue entry, which is a different screen.
 */
const addableMaps = computed<UnresolvedName[]>(() =>
  (preview.value?.unresolved ?? []).filter((u) => u.reason === 'MAP_NOT_IN_POOL' && u.mapId != null),
)

async function runImport() {
  if (!canImport.value) return
  loading.value = true
  error.value = null
  violations.value = []
  preview.value = null

  try {
    preview.value = await api.admin.importMatch(props.tournamentId, sourceUrl.value.trim())
  } catch (e) {
    error.value = describeError(e, 'Failed to import that match')
    violations.value = violationMessages(e)
  } finally {
    loading.value = false
  }
}

/**
 * Adds a scraped board to this tournament's pool, then re-imports so the
 * preview reflects it. The admin clicks this — the importer never widens a
 * board pool on its own.
 */
async function addMapToPool(unresolved: UnresolvedName) {
  if (unresolved.mapId == null) return
  addingMapId.value = unresolved.mapId
  error.value = null
  try {
    await api.admin.addMapToPool(props.tournamentId, unresolved.mapId)
    await runImport()
  } catch (e) {
    error.value = describeError(e, `Failed to add ${unresolved.sourceName} to the board pool`)
    violations.value = violationMessages(e)
  } finally {
    addingMapId.value = null
  }
}

function playerLabels(p: MatchImportPreviewDto): string {
  return p.participants.map((side) => side.playerLabel || 'Unnamed').join('  vs  ')
}
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between gap-4">
      <h2 class="headline text-lg">Import match from URL</h2>
      <button type="button" class="btn-ghost" @click="emit('cancel')">Cancel</button>
    </div>

    <ErrorBanner :message="error" :violations="violations" />

    <div class="panel p-4 md:p-6 space-y-4">
      <label class="label-caps block" for="import-source-url">Match page URL</label>
      <div class="flex flex-col gap-3 sm:flex-row">
        <input
          id="import-source-url"
          v-model="sourceUrl"
          type="url"
          class="field-input flex-1"
          placeholder="https://www.tabletopleague.com/o/<org>/<competition>/matches/<id>"
          :disabled="loading"
          @keyup.enter="runImport"
        />
        <button type="button" class="btn-primary sm:w-40" :disabled="!canImport" @click="runImport">
          {{ loading ? 'Reading…' : 'Import' }}
        </button>
      </div>
      <p class="text-xs text-ink-dim">
        Paste the link to a single completed match. Reading it takes a few seconds — the source page
        is rendered in a real browser before it can be read.
      </p>
    </div>

    <div v-if="preview" class="space-y-4">
      <!-- Duplicate warning: a link is not unique, so this informs rather than blocks. -->
      <div
        v-if="preview.alreadyImportedMatchId"
        class="panel border-magenta/50 p-4"
      >
        <p class="label-caps text-magenta">Already imported</p>
        <p class="mt-1 text-sm text-ink-dim">
          This tournament already has a match recorded against this URL (match
          #{{ preview.alreadyImportedMatchId }}). Importing again will create a second one — correct
          the existing match instead if that's what you meant.
        </p>
      </div>

      <div class="panel p-4 md:p-6 space-y-3">
        <p class="headline text-base">{{ playerLabels(preview) }}</p>
        <dl class="grid grid-cols-2 gap-x-6 gap-y-2 text-sm md:grid-cols-4">
          <div>
            <dt class="label-caps">Round</dt>
            <dd class="text-ink-dim">{{ preview.roundName ?? '—' }}</dd>
          </div>
          <div>
            <dt class="label-caps">Format</dt>
            <dd class="text-ink-dim">{{ preview.seriesFormat ?? '—' }}</dd>
          </div>
          <div>
            <dt class="label-caps">Games</dt>
            <dd class="text-ink-dim">{{ preview.games.length }}</dd>
          </div>
          <div>
            <dt class="label-caps">Played</dt>
            <dd class="text-ink-dim">
              {{ preview.playedAt ? new Date(preview.playedAt).toLocaleString() : (preview.playedAtRaw ?? '—') }}
            </dd>
          </div>
        </dl>
        <p class="text-xs text-ink-dim">
          The source names its rounds rather than numbering them, so you'll set the round number in
          the next step.<span v-if="!preview.playedAt">
            Its timestamp couldn't be read either — check the date before saving.</span>
        </p>
      </div>

      <!-- Unresolved names. Nothing here is guessed; each row is the admin's call. -->
      <div v-if="preview.unresolved.length > 0" class="panel border-magenta/50 p-4 md:p-6 space-y-3">
        <p class="label-caps text-magenta">
          {{ preview.unresolved.length }} name{{ preview.unresolved.length === 1 ? '' : 's' }} couldn't be matched
        </p>
        <ul class="space-y-2">
          <li
            v-for="item in preview.unresolved"
            :key="`${item.kind}-${item.sourceName}`"
            class="flex flex-col gap-2 border border-edge bg-surface-lowest p-3 sm:flex-row sm:items-center sm:justify-between"
          >
            <div class="min-w-0">
              <p class="text-sm">
                <span class="label-caps mr-2">{{ item.kind }}</span>
                <span class="font-medium">{{ item.sourceName }}</span>
              </p>
              <p class="text-xs text-ink-dim">{{ item.message }}</p>
            </div>
            <button
              v-if="item.reason === 'MAP_NOT_IN_POOL'"
              type="button"
              class="btn-ghost shrink-0"
              :disabled="addingMapId !== null || loading"
              @click="addMapToPool(item)"
            >
              {{ addingMapId === item.mapId ? 'Adding…' : 'Add to board pool' }}
            </button>
          </li>
        </ul>
        <p v-if="addableMaps.length === 0" class="text-xs text-ink-dim">
          Add the missing entries under Heroes or Maps, then import again.
        </p>
      </div>

      <div class="flex flex-col gap-3 sm:flex-row sm:justify-end">
        <button type="button" class="btn-ghost" @click="emit('cancel')">Cancel</button>
        <button
          type="button"
          class="btn-primary"
          :disabled="!isResolved"
          @click="emit('review', preview)"
        >
          Review and record
        </button>
      </div>
      <p v-if="!isResolved" class="text-right text-xs text-ink-dim">
        Every name has to resolve before this match can be recorded.
      </p>
    </div>
  </div>
</template>
