<script setup lang="ts">
import type { BanType, Hero } from '@/api/types'
import * as matchForm from '@/domain/matchForm'
import type { MatchForm } from '@/domain/matchForm'

/**
 * The bans a saved match carries with no side, from before `hero_ban.side`
 * existed.
 *
 * They are neither dropped nor guessed: the ban already scored points, so
 * losing it would silently move the standings. Placing one unions its hero back
 * onto that side's arsenal, which is why the assignment goes through the domain
 * module. Nothing below can be filled in sensibly until they are placed, so this
 * renders above the drafts and `matchForm.validate` holds the save.
 */

const form = defineModel<MatchForm>({ required: true })

const props = defineProps<{
  heroPool: Hero[]
}>()

const banTypeLabels: Record<BanType, string> = {
  PRE_BAN: 'Pre-ban',
  OPPONENT_BAN: 'Opponent ban',
  SELF_BAN: 'Self ban',
}

const heroName = (heroId: number) => matchForm.heroName(props.heroPool, heroId)
const assignBanToSide = (index: number, side: number) =>
  matchForm.assignBanToSide(form.value, index, side)
</script>

<template>
  <div class="flex flex-col gap-3 border border-magenta bg-surface-lowest p-4">
    <h4 class="headline text-base text-magenta">Bans with no side</h4>
    <p class="font-mono text-xs leading-relaxed text-ink-dim">
      This match was recorded before a ban stored which draft it came out of. Place each one on
      the side whose hero it was — the type already says who struck it.
    </p>
    <div
      v-for="(ban, index) in form.unassignedBans"
      :key="index"
      class="flex flex-col gap-2 border border-edge bg-surface-low p-3 sm:flex-row sm:items-center sm:justify-between"
    >
      <span class="font-mono text-sm">
        {{ heroName(ban.heroId) }}
        <span class="text-xs text-ink-dim">({{ banTypeLabels[ban.banType] }})</span>
      </span>
      <div class="flex gap-2">
        <button
          v-for="side in [0, 1]"
          :key="side"
          type="button"
          class="btn-ghost px-4 py-2 text-xs"
          @click="assignBanToSide(index, side)"
        >
          Side {{ side + 1 }}
        </button>
      </div>
    </div>
  </div>
</template>
