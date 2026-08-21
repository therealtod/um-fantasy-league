<script setup lang="ts">
import { computed } from 'vue'
import type { Hero, MapAdminDto } from '@/api/types'
import * as matchForm from '@/domain/matchForm'
import type { MatchForm } from '@/domain/matchForm'

/**
 * One game of the series: the board it was played on, and each side's hero,
 * health and winner flag.
 *
 * Every hero dropdown here offers only what that side drafted and did not lose
 * to a ban, which is what makes `PLAYED_HERO_NOT_DRAFTED` and
 * `BANNED_HERO_PLAYED` unreachable from this form.
 */

const form = defineModel<MatchForm>({ required: true })

const props = defineProps<{
  heroPool: Hero[]
  mapPool: MapAdminDto[]
  /** This game's position in the series — the form renumbers 1..N on removal. */
  index: number
}>()

const game = computed(() => form.value.games[props.index])

const fieldableHeroes = (side: number) => matchForm.fieldableHeroes(form.value, props.heroPool, side)

/* Both of these carry an invariant past the row they touch — a dense 1..N
   renumbering and the winner's exclusivity — so they live in the domain module. */
const removeGame = () => matchForm.removeGame(form.value, props.index)
const setWinner = (participantIndex: number) =>
  matchForm.setWinner(form.value, props.index, participantIndex)
</script>

<template>
  <div class="flex flex-col gap-4 border border-edge bg-surface-low p-4">
    <div class="flex items-center justify-between">
      <h5 class="label-caps text-cyan">Game {{ game.gameNumber }}</h5>
      <button
        v-if="form.games.length > 1"
        type="button"
        class="btn-ghost px-4 py-2 text-xs"
        @click="removeGame"
      >
        Remove Game
      </button>
    </div>

    <div class="flex flex-col gap-2">
      <label :for="`game-${index}-map`" class="label-caps">Map</label>
      <select
        :id="`game-${index}-map`"
        v-model.number="game.mapId"
        class="field-input-sm"
      >
        <option :value="0" disabled>Select a map…</option>
        <option v-for="map in mapPool" :key="map.id" :value="map.id">{{ map.name }}</option>
      </select>
    </div>

    <div
      v-for="(participant, participantIndex) in game.participants"
      :key="participantIndex"
      class="grid grid-cols-2 gap-4 border border-edge bg-surface-lowest p-3 sm:grid-cols-4"
    >
      <div class="flex flex-col gap-2">
        <label :for="`game-${index}-hero-${participantIndex}`" class="label-caps">
          Side {{ participantIndex + 1 }} Hero
        </label>
        <!-- Only what this side drafted and did not lose to a ban, which is
             what makes PLAYED_HERO_NOT_DRAFTED unreachable from this form. -->
        <select
          :id="`game-${index}-hero-${participantIndex}`"
          v-model.number="participant.heroId"
          class="field-input-sm"
        >
          <option :value="0" disabled>
            {{
              fieldableHeroes(participantIndex).length
                ? 'Select a hero…'
                : `Draft heroes for side ${participantIndex + 1} first`
            }}
          </option>
          <option v-for="hero in fieldableHeroes(participantIndex)" :key="hero.id" :value="hero.id">
            {{ hero.name }}
          </option>
        </select>
      </div>

      <div class="flex flex-col gap-2">
        <label :for="`game-${index}-health-${participantIndex}`" class="label-caps">Health (loser: 0 or less)</label>
        <input
          :id="`game-${index}-health-${participantIndex}`"
          v-model.number="participant.healthRemaining"
          type="number"
          class="field-input-sm"
        />
      </div>

      <div class="flex flex-col gap-2">
        <!-- A radio, not a checkbox: exactly one side wins each game, and
             there is no "neither" to express. -->
        <label class="flex cursor-pointer items-center gap-2 pt-5 font-mono text-xs text-ink">
          <input
            type="radio"
            :name="`game-${index}-winner`"
            :checked="participant.isWinner"
            @change="setWinner(participantIndex)"
          />
          <span>Winner</span>
        </label>
      </div>
    </div>
  </div>
</template>
