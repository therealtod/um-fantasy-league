<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Hero, RosterRole } from '@/api/types'
import * as matchForm from '@/domain/matchForm'
import type { MatchForm } from '@/domain/matchForm'
import { parseStatsFile } from '@/domain/matchStats'

/**
 * The match's stats sheet: how often each hero attacked, healed, schemed in each
 * game, uploaded as a CSV or JSON file. A manager's role for a hero is what turns
 * these counts into a role bonus, so this section only appears in a tournament
 * that uses roles.
 *
 * Reading the file is `matchStats.parseStatsFile`; placing its rows onto the
 * games is `matchForm.applyStats`. Both report every problem at once, and a
 * file with any problem changes nothing.
 */

const form = defineModel<MatchForm>({ required: true })

const props = defineProps<{
  heroPool: Hero[]
  /** The tournament's roles, so the section can flag a stat no role rewards. */
  roles: RosterRole[]
}>()

const problems = ref<string[]>([])
const loadedFile = ref<string | null>(null)

const columns = computed(() => matchForm.statNames(form.value))

/** Stats on the sheet that no role weights — they are recorded, but earn nothing. */
const unrewarded = computed(() => {
  const weighted = new Set(props.roles.flatMap((role) => role.weights.map((w) => w.stat)))
  return columns.value.filter((stat) => !weighted.has(stat))
})

const rows = computed(() =>
  form.value.games.flatMap((game) =>
    game.participants.map((participant, side) => ({
      key: `${game.gameNumber}-${side}`,
      gameNumber: game.gameNumber,
      side: side + 1,
      hero: participant.heroId ? matchForm.heroName(props.heroPool, participant.heroId) : '—',
      stats: participant.stats ?? {},
    })),
  ),
)

async function onFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  const parsed = parseStatsFile(await file.text(), file.name)
  problems.value = parsed.ok
    ? matchForm.applyStats(form.value, parsed.rows, props.heroPool)
    : parsed.errors
  loadedFile.value = problems.value.length === 0 ? file.name : null
  // So choosing the same file again — after fixing it — fires another change.
  input.value = ''
}

function clear() {
  matchForm.clearStats(form.value)
  problems.value = []
  loadedFile.value = null
}
</script>

<template>
  <div class="flex flex-col gap-4 border border-edge bg-surface-lowest p-4">
    <h4 class="headline text-base text-cyan">Game Stats (optional)</h4>
    <p class="font-mono text-xs leading-relaxed text-ink-dim">
      Upload the match's stats sheet — one row per hero per game, as CSV
      (<code>game,hero,ATTACKS,HEALING,…</code>) or JSON
      (<code>[{"game": 1, "hero": "…", "stats": {"ATTACKS": 5}}]</code>). Choose each game's
      heroes first: rows are matched to the heroes who played. The role each manager gave a hero
      decides what its stats are worth.
    </p>

    <div class="flex flex-wrap items-center gap-3">
      <input
        id="match-stats-file"
        type="file"
        accept=".csv,.json,text/csv,application/json"
        class="sr-only"
        @change="onFile"
      />
      <label for="match-stats-file" class="btn-ghost cursor-pointer">Upload Stats File</label>
      <button v-if="columns.length" type="button" class="btn-ghost" @click="clear">
        Clear Stats
      </button>
      <span v-if="loadedFile" class="font-mono text-xs text-lime">Applied {{ loadedFile }}</span>
    </div>

    <div
      v-if="problems.length"
      class="flex flex-col gap-1 border border-danger/50 bg-danger/10 p-3 font-mono text-xs text-danger"
    >
      <p class="font-bold">The file was not applied:</p>
      <p v-for="problem in problems" :key="problem">{{ problem }}</p>
    </div>

    <template v-if="columns.length">
      <div class="overflow-x-auto">
        <table class="w-full font-mono text-xs">
          <thead>
            <tr class="text-left text-ink-dim">
              <th class="px-2 py-1">Game</th>
              <th class="px-2 py-1">Side</th>
              <th class="px-2 py-1">Hero</th>
              <th v-for="stat in columns" :key="stat" class="px-2 py-1 text-right">{{ stat }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in rows" :key="row.key" class="border-t border-edge">
              <td class="px-2 py-1">{{ row.gameNumber }}</td>
              <td class="px-2 py-1">{{ row.side }}</td>
              <td class="px-2 py-1 text-ink">{{ row.hero }}</td>
              <td v-for="stat in columns" :key="stat" class="px-2 py-1 text-right text-cyan">
                {{ row.stats[stat] ?? '—' }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <p v-if="unrewarded.length" class="font-mono text-xs text-ink-dim">
        No role rewards {{ unrewarded.join(', ') }} — recorded, but worth nothing until a role
        weights {{ unrewarded.length === 1 ? 'it' : 'them' }}.
      </p>
    </template>
  </div>
</template>
