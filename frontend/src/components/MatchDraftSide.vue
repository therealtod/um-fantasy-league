<script setup lang="ts">
import { computed } from 'vue'
import type { Hero } from '@/api/types'
import * as matchForm from '@/domain/matchForm'
import type { MatchForm, SidedBanType } from '@/domain/matchForm'

/**
 * One side's draft: who piloted it, its whole arsenal, and the heroes struck out
 * of that arsenal.
 *
 * The arsenal is the list every dropdown below the draft filters down to, so
 * this block is what the games and the pre-bans are downstream of — see
 * `matchForm.ts` for why the form holds picks and bans together where the API
 * keeps them disjoint.
 *
 * It takes the whole form rather than just its own side because the option
 * lists are form-wide rules, not per-side ones, and they live in the domain
 * module where they can be tested as data.
 */

const form = defineModel<MatchForm>({ required: true })

const props = defineProps<{
  heroPool: Hero[]
  /** This side's list position, which is what `side` means everywhere else too. */
  side: number
}>()

const sidedBanTypes: { value: SidedBanType; label: string }[] = [
  { value: 'OPPONENT_BAN', label: 'Opponent ban' },
  { value: 'SELF_BAN', label: 'Self ban' },
]

const sideForm = computed(() => form.value.sides[props.side])

const draftableHeroes = (currentHeroId: number) =>
  matchForm.draftableHeroes(form.value, props.heroPool, props.side, currentHeroId)
const bannableHeroes = (currentHeroId: number) =>
  matchForm.bannableHeroes(form.value, props.heroPool, props.side, currentHeroId)

function addDraftPick() {
  sideForm.value.draftedHeroIds.push(0)
}

/* A removal here cascades to the games and bans that named the hero, so it goes
   through the domain module rather than a splice. */
const removeDraftPick = (index: number) => matchForm.removeDraftPick(form.value, props.side, index)

function addSideBan() {
  sideForm.value.bans.push({ heroId: 0, banType: 'OPPONENT_BAN' })
}

function removeSideBan(index: number) {
  sideForm.value.bans.splice(index, 1)
}
</script>

<template>
  <div class="flex flex-col gap-4 border border-edge bg-surface-low p-4">
    <div class="flex flex-col gap-4 sm:flex-row sm:items-center">
      <div class="flex size-8 shrink-0 items-center justify-center bg-cyan font-display text-xl text-surface-lowest">
        {{ side + 1 }}
      </div>
      <div class="flex flex-1 flex-col gap-2">
        <label :for="`player-${side}`" class="label-caps">Player name (optional)</label>
        <input
          :id="`player-${side}`"
          v-model="sideForm.playerLabel"
          type="text"
          class="field-input-sm"
          placeholder="Who piloted this side"
        />
      </div>
    </div>

    <div class="flex flex-col gap-3 border-t border-edge pt-3">
      <span class="label-caps">Drafted heroes</span>

      <div
        v-for="(_heroId, pickIndex) in sideForm.draftedHeroIds"
        :key="pickIndex"
        class="flex flex-col gap-2 sm:flex-row sm:items-end"
      >
        <div class="flex flex-1 flex-col gap-2">
          <label :for="`draft-${side}-${pickIndex}`" class="label-caps">Hero {{ pickIndex + 1 }}</label>
          <select
            :id="`draft-${side}-${pickIndex}`"
            v-model.number="sideForm.draftedHeroIds[pickIndex]"
            class="field-input-sm"
          >
            <option :value="0" disabled>Select a hero…</option>
            <option
              v-for="hero in draftableHeroes(sideForm.draftedHeroIds[pickIndex])"
              :key="hero.id"
              :value="hero.id"
            >
              {{ hero.name }}
            </option>
          </select>
        </div>
        <button type="button" class="btn-ghost px-4 py-2 text-xs" @click="removeDraftPick(pickIndex)">
          Remove
        </button>
      </div>

      <button type="button" class="btn-ghost" @click="addDraftPick">+ Add Drafted Hero</button>
    </div>

    <!-- This side's bans: heroes struck out of the arsenal above. They score a
         ban instead of an appearance, which is why they belong on the draft
         rather than beside it. -->
    <div class="flex flex-col gap-3 border-t border-edge pt-3">
      <span class="label-caps">Struck out of this draft (optional)</span>

      <div
        v-for="(ban, banIndex) in sideForm.bans"
        :key="banIndex"
        class="flex flex-col gap-4 sm:flex-row sm:items-end"
      >
        <div class="grid flex-1 grid-cols-2 gap-4">
          <div class="flex flex-col gap-2">
            <label :for="`ban-hero-${side}-${banIndex}`" class="label-caps">Hero</label>
            <select
              :id="`ban-hero-${side}-${banIndex}`"
              v-model.number="ban.heroId"
              class="field-input-sm"
            >
              <option :value="0" disabled>
                {{ sideForm.draftedHeroIds.length ? 'Select a hero…' : `Draft heroes for side ${side + 1} first` }}
              </option>
              <option v-for="hero in bannableHeroes(ban.heroId)" :key="hero.id" :value="hero.id">
                {{ hero.name }}
              </option>
            </select>
          </div>

          <div class="flex flex-col gap-2">
            <label :for="`ban-type-${side}-${banIndex}`" class="label-caps">Struck by</label>
            <select
              :id="`ban-type-${side}-${banIndex}`"
              v-model="ban.banType"
              class="field-input-sm"
            >
              <option v-for="type in sidedBanTypes" :key="type.value" :value="type.value">{{ type.label }}</option>
            </select>
          </div>
        </div>

        <button type="button" class="btn-ghost px-4 py-2 text-xs" @click="removeSideBan(banIndex)">
          Remove
        </button>
      </div>

      <button type="button" class="btn-ghost" @click="addSideBan">+ Add Ban</button>
    </div>
  </div>
</template>
