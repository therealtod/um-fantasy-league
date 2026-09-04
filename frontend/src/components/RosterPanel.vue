<script setup lang="ts">
import { computed, ref } from 'vue'
import BudgetMeter from './BudgetMeter.vue'
import DestructiveConfirmPanel from './DestructiveConfirmPanel.vue'
import { useRosterStore } from '@/stores/roster'
import { lockBlockedReason } from '@/domain/rosterGuidance'
import { formatCredits } from '@/lib/format'

const roster = useRosterStore()

const emptySlots = computed(() => Math.max(0, roster.rosterSize - roster.selected.length))

const averageCost = computed(() =>
  roster.selected.length === 0 ? 0 : Math.round(roster.budget.spent / roster.selected.length),
)

/** Why the lock button is dead, so a disabled button always explains itself. */
const blockedReason = computed(() =>
  lockBlockedReason({
    registered: roster.registered,
    locked: roster.locked,
    picked: roster.selected.length,
    rosterSize: roster.rosterSize,
    remaining: roster.budget.remaining,
    creditGrant: roster.creditGrant,
  }),
)

/**
 * Locking is irreversible — there is no unlock endpoint — so the button asks
 * before it acts. The confirmation replaces the footer rather than opening a
 * dialog, which keeps the roster it is about to freeze on screen.
 */
const confirming = ref(false)

async function confirmLock() {
  await roster.lock()
  // On success the panel disappears with `lockable`; on refusal it stays open
  // so the confirm button can be pressed again once the error banner's
  // violations (rendered by the view) have been dealt with.
  if (roster.locked) confirming.value = false
}
</script>

<template>
  <aside class="panel flex w-full flex-col lg:w-80 lg:shrink-0 lg:self-start">
    <header class="border-b border-edge px-5 py-4">
      <h3 class="headline text-base uppercase">Your Roster</h3>
      <p class="label-caps mt-1">
        {{
          roster.locked ? 'Locked' : `${roster.selected.length} of ${roster.rosterSize} heroes picked`
        }}
      </p>
    </header>

    <div class="border-b border-edge px-5 py-4">
      <BudgetMeter :budget="roster.budget" />
    </div>

    <!-- Slots -->
    <ul class="space-y-2 px-5 py-4">
      <li
        v-for="(hero, index) in roster.selected"
        :key="hero.id"
        class="flex items-center gap-3 border border-edge bg-surface-mid p-2.5"
      >
        <span class="label-caps w-4 shrink-0">{{ index + 1 }}</span>
        <p class="min-w-0 flex-1 truncate font-mono text-xs font-bold text-ink uppercase">
          {{ hero.name }}
        </p>
        <span class="stat-value shrink-0 text-xs text-cyan">{{ formatCredits(hero.cost) }}</span>
        <button
          v-if="!roster.locked"
          type="button"
          class="shrink-0 px-1 font-mono text-xs text-ink-dim transition-colors hover:text-magenta"
          :aria-label="`Remove ${hero.name}`"
          @click="roster.toggle(hero.id)"
        >
          &times;
        </button>
      </li>

      <li
        v-for="slot in emptySlots"
        :key="`empty-${slot}`"
        class="flex items-center justify-center border border-dashed border-edge bg-surface-lowest/50 p-4"
      >
        <span class="label-caps">Empty slot</span>
      </li>
    </ul>

    <!-- Aggregates -->
    <div class="border-t border-edge px-5 py-4">
      <div class="border border-edge bg-surface-mid p-2.5">
        <p class="label-caps">Avg Cost</p>
        <p class="stat-value mt-1 text-sm text-ink">{{ formatCredits(averageCost) }}</p>
      </div>
    </div>

    <div class="border-t border-edge p-4">
      <DestructiveConfirmPanel
        v-if="confirming && roster.lockable"
        title="Lock your roster?"
        confirm-label="Lock Roster"
        busy-label="Locking…"
        :busy="roster.saving"
        @cancel="confirming = false"
        @confirm="confirmLock"
      >
        This is final — you cannot change your picks afterwards. Leaving the roster unlocked is
        worse: an unlocked entry is removed when the tournament goes live, and scores nothing.
      </DestructiveConfirmPanel>

      <template v-else-if="!roster.locked">
        <button
          class="btn-primary w-full"
          :disabled="!roster.lockable || roster.saving"
          @click="confirming = true"
        >
          <template v-if="roster.saving">Working…</template>
          <template v-else>
            Lock In Roster ({{ roster.selected.length }}/{{ roster.rosterSize }})
          </template>
        </button>
        <p v-if="blockedReason" class="mt-2 font-mono text-[11px] text-ink-dim">
          {{ blockedReason }}
        </p>
      </template>

      <div v-else class="space-y-3">
        <p
          class="border border-lime/50 bg-lime/10 py-3 text-center font-mono text-xs tracking-[0.1em] text-lime uppercase"
        >
          Roster Locked
        </p>
        <RouterLink class="btn-ghost block w-full text-center" to="/standings">
          View Standings
        </RouterLink>
      </div>
    </div>
  </aside>
</template>
