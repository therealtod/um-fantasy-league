<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { api } from '@/api/client'
import { usePoolManagement } from '@/composables/usePoolManagement'
import { byName } from '@/lib/sort'
import type { Hero, HeroAdminDto } from '@/api/types'
import ErrorBanner from '@/components/ErrorBanner.vue'
import DestructiveConfirmPanel from '@/components/DestructiveConfirmPanel.vue'
import TournamentSelect from '@/components/TournamentSelect.vue'

const {
  tournaments,
  selectedTournamentId,
  catalog: heroes,
  pool: heroPoolHeroes,
  loading,
  error,
  violations,
  run,
  reloadPool,
  removingItem: removingHero,
  startRemove: startRemoveHero,
  cancelRemove: cancelRemoveHero,
  confirmRemove: confirmRemoveHero,
} = usePoolManagement<HeroAdminDto, Hero>({
  entityLabel: 'Hero',
  loadCatalog: () => api.admin.listHeroes().then((list) => list.sort(byName)),
  loadPool: (tournamentId) => api.admin.listHeroPool(tournamentId),
  removeFromPool: (tournamentId, heroId) => api.admin.removeHeroFromPool(tournamentId, heroId),
})

const showAddForm = ref(false)

const addForm = ref({
  heroId: null as number | null,
  cost: 1000,
})

// Heroes staged for the next batch submit — built up client-side, sent in one request.
const pendingHeroes = ref<{ heroId: number; name: string; cost: number }[]>([])

// Heroes not yet in this tournament's pool and not already staged — the only ones worth adding.
const availableHeroes = computed(() => {
  const poolIds = new Set(heroPoolHeroes.value.map((h) => h.id))
  const pendingIds = new Set(pendingHeroes.value.map((p) => p.heroId))
  return heroes.value.filter((h) => !poolIds.has(h.id) && !pendingIds.has(h.id))
})

// The add form and its staged batch are local to this screen — reset them on
// a tournament switch, alongside usePoolManagement's own reset of the pool
// and the pending removal.
watch(selectedTournamentId, () => {
  cancelAddForm()
})

function startAddHero() {
  addForm.value = { heroId: null, cost: 1000 }
  pendingHeroes.value = []
  showAddForm.value = true
}

function cancelAddForm() {
  showAddForm.value = false
  addForm.value = { heroId: null, cost: 1000 }
  pendingHeroes.value = []
}

// Stages one hero locally — no API call yet. The whole staged list ships in one request from submitBatch.
function stageHero() {
  if (!addForm.value.heroId) {
    error.value = 'Please select a hero and enter a cost'
    return
  }
  const hero = heroes.value.find((h) => h.id === addForm.value.heroId)
  if (!hero) return

  error.value = null
  violations.value = []
  pendingHeroes.value.push({ heroId: hero.id, name: hero.name, cost: addForm.value.cost })
  addForm.value = { heroId: null, cost: 1000 }
}

function removePendingHero(index: number) {
  pendingHeroes.value.splice(index, 1)
}

// Draft value for each pool row's cost input, keyed by hero id and rebuilt
// whenever the pool is (re)loaded. A rejected edit reverts through this
// reactive draft instead of writing to the input's DOM value directly, and
// an accepted edit is folded back in from setHeroCost's own response
// instead of refetching the whole pool for a single-field change.
// `<input type="number">`'s v-model casts through Vue's own number
// coercion, so a draft is a number once parseable and stays the raw string
// (typically `''`) while it isn't — see onCostInputChange below.
const costDrafts = ref<Record<number, number | string>>({})

watch(
  heroPoolHeroes,
  (pool) => {
    costDrafts.value = Object.fromEntries(pool.map((hero) => [hero.id, hero.cost]))
  },
  { immediate: true },
)

async function submitBatch() {
  if (!selectedTournamentId.value || pendingHeroes.value.length === 0) return
  const tournamentId = selectedTournamentId.value

  const result = await run(
    () =>
      api.admin.addHeroesToPool(
        tournamentId,
        pendingHeroes.value.map((p) => ({ heroId: p.heroId, cost: p.cost })),
      ),
    'Failed to add heroes to pool',
  )
  if (!result.ok) return
  await reloadPool(tournamentId)
  cancelAddForm()
}

async function onCostInputChange(hero: Hero) {
  const draft = costDrafts.value[hero.id]
  const newCost = typeof draft === 'number' ? draft : Number(draft)

  if (draft === '' || !Number.isFinite(newCost) || newCost < 1) {
    error.value = 'Cost must be a number of at least 1 credit'
    violations.value = []
    costDrafts.value[hero.id] = hero.cost
    return
  }

  if (!selectedTournamentId.value) return
  const tournamentId = selectedTournamentId.value

  const result = await run(
    () => api.admin.setHeroCost(tournamentId, hero.id, newCost),
    'Failed to update hero cost',
  )
  if (!result.ok) {
    costDrafts.value[hero.id] = hero.cost
    return
  }
  // setHeroCost's response is this one row's new state — no need to refetch
  // the whole pool for a single-field edit. Mutated in place (not replaced)
  // so the `hero` this closure and the next edit's revert both see it.
  hero.cost = result.value.cost
  costDrafts.value[hero.id] = hero.cost
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">Hero Pool & Pricing</h2>
    </div>

    <ErrorBanner :message="error" :violations="violations" />

    <!-- Tournament selector -->
    <TournamentSelect v-model="selectedTournamentId" :tournaments="tournaments" />

    <!-- Hero pool controls -->
    <div v-if="selectedTournamentId && !showAddForm && !removingHero" class="flex gap-3">
      <button class="btn-primary" @click="startAddHero">+ Add Hero to Pool</button>
    </div>

    <!-- Add heroes form — stage several picks, then submit once as a batch -->
    <div v-if="showAddForm" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">Add Heroes to Pool</h3>

      <div class="flex flex-wrap items-end gap-3">
        <div class="flex flex-1 flex-col gap-2">
          <label for="hero-select" class="label-caps">Hero</label>
          <select
            id="hero-select"
            v-model.number="addForm.heroId"
            class="cursor-pointer field-input"
          >
            <option :value="null">-- Choose a hero --</option>
            <option v-for="hero in availableHeroes" :key="hero.id" :value="hero.id">
              {{ hero.name }}
            </option>
          </select>
        </div>

        <div class="flex flex-col gap-2">
          <label for="hero-cost-input" class="label-caps">Cost (Credits)</label>
          <input
            id="hero-cost-input"
            v-model.number="addForm.cost"
            type="number"
            min="1"
            class="w-32 field-input"
            placeholder="e.g., 5000"
          />
        </div>

        <button class="btn-primary" :disabled="!addForm.heroId" @click="stageHero">+ Stage Hero</button>
      </div>

      <p v-if="heroes.length === 0" class="font-mono text-xs text-ink-dim italic">
        No heroes exist yet. Create one in the Heroes section first.
      </p>
      <p v-else-if="availableHeroes.length === 0 && pendingHeroes.length === 0" class="font-mono text-xs text-ink-dim italic">
        Every hero is already in this tournament's pool.
      </p>

      <!-- Staged heroes — nothing is sent to the server until "Add ... to Pool" below -->
      <div v-if="pendingHeroes.length > 0" class="flex flex-col gap-2">
        <p class="label-caps">Staged for this batch ({{ pendingHeroes.length }})</p>
        <div
          v-for="(pending, index) in pendingHeroes"
          :key="pending.heroId"
          class="flex items-center justify-between gap-3 border border-edge bg-surface-lowest px-3 py-2"
        >
          <span class="font-mono text-sm text-ink">{{ pending.name }}</span>
          <div class="flex items-center gap-2">
            <input
              v-model.number="pending.cost"
              type="number"
              min="1"
              class="w-24 field-input-sm"
            />
            <span class="font-mono text-xs text-ink-dim">CR</span>
            <button
              class="border border-magenta px-3 py-1 font-mono text-xs text-magenta transition-opacity hover:opacity-85"
              @click="removePendingHero(index)"
            >
              Remove
            </button>
          </div>
        </div>
      </div>

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="cancelAddForm">Cancel</button>
        <button class="btn-primary" :disabled="loading || pendingHeroes.length === 0" @click="submitBatch">
          {{
            loading
              ? 'Adding...'
              : `Add ${pendingHeroes.length} ${pendingHeroes.length === 1 ? 'Hero' : 'Heroes'} to Pool`
          }}
        </button>
      </div>
    </div>

    <!-- Remove confirmation — costs are joined live, so dropping the row re-prices unlocked rosters -->
    <DestructiveConfirmPanel
      v-if="removingHero"
      title="Remove Hero from Pool"
      confirm-label="Remove from Pool"
      busy-label="Removing..."
      :busy="loading"
      @cancel="cancelRemoveHero"
      @confirm="confirmRemoveHero"
    >
      Remove <strong>{{ removingHero.name }}</strong> from this tournament's pool? Any unlocked
      roster still holding this hero will re-price it to 0 credits until it is picked again.
    </DestructiveConfirmPanel>

    <!-- Hero pool list -->
    <div v-if="selectedTournamentId && !showAddForm && !removingHero" class="flex flex-col gap-2">
      <div v-if="heroPoolHeroes.length === 0" class="p-12 text-center">
        <p class="text-ink-dim">No heroes in this tournament's pool yet. Add heroes to begin.</p>
      </div>

      <div
        v-for="hero in heroPoolHeroes"
        :key="hero.id"
        class="panel flex flex-wrap items-center justify-between gap-3 p-4"
      >
        <div class="flex flex-col gap-1">
          <span class="headline text-base text-ink">{{ hero.name }}</span>
          <span class="font-mono text-xs text-ink-dim">ID: {{ hero.id }}</span>
        </div>
        <div class="flex items-center gap-2">
          <label :for="`cost-${hero.id}`" class="label-caps">Cost:</label>
          <input
            :id="`cost-${hero.id}`"
            v-model="costDrafts[hero.id]"
            type="number"
            min="1"
            class="w-24 field-input-sm"
            @change="onCostInputChange(hero)"
          />
          <span class="font-mono text-xs text-ink-dim">CR</span>
          <button
            class="border border-magenta px-4 py-1.5 font-mono text-xs text-magenta transition-opacity hover:opacity-85"
            @click="startRemoveHero(hero)"
          >
            Remove
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
