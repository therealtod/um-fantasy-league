<script setup lang="ts">
import type { Hero } from '@/api/types'
import * as matchForm from '@/domain/matchForm'
import type { MatchForm } from '@/domain/matchForm'

/**
 * The pre-bans: heroes struck before sides were assigned, so they belong to
 * neither draft and score neither ban metric.
 *
 * That is also why the dropdown offers only heroes nobody drafted — offering a
 * drafted one would be offering a `BANNED_HERO_DRAFTED` 422.
 */

const form = defineModel<MatchForm>({ required: true })

const props = defineProps<{
  heroPool: Hero[]
}>()

const preBannableHeroes = (currentHeroId: number) =>
  matchForm.preBannableHeroes(form.value, props.heroPool, currentHeroId)

function addPreBan() {
  form.value.preBans.push({ heroId: 0 })
}

function removePreBan(index: number) {
  form.value.preBans.splice(index, 1)
}
</script>

<template>
  <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
    <h4 class="headline text-base text-cyan">Pre-bans (optional)</h4>
    <p class="font-mono text-xs leading-relaxed text-ink-dim">
      Heroes struck before sides were known. They belong to neither draft, so only heroes nobody
      drafted are offered.
    </p>

    <div
      v-for="(ban, index) in form.preBans"
      :key="index"
      class="flex flex-col gap-4 border border-edge bg-surface-low p-4 sm:flex-row sm:items-end"
    >
      <div class="flex flex-1 flex-col gap-2">
        <label :for="`pre-ban-hero-${index}`" class="label-caps">Hero</label>
        <select
          :id="`pre-ban-hero-${index}`"
          v-model.number="ban.heroId"
          class="field-input-sm"
        >
          <option :value="0" disabled>Select a hero…</option>
          <option v-for="hero in preBannableHeroes(ban.heroId)" :key="hero.id" :value="hero.id">
            {{ hero.name }}
          </option>
        </select>
      </div>

      <button type="button" class="btn-ghost px-4 py-2 text-xs" @click="removePreBan(index)">
        Remove
      </button>
    </div>

    <button type="button" class="btn-ghost" @click="addPreBan">+ Add Pre-ban</button>
  </div>
</template>
