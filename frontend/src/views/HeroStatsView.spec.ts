import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { HeroRankRow, HeroStatsBoard, Tournament } from '@/api/types'

vi.mock('@/api/client', () => ({
  api: { heroStats: vi.fn(), tournaments: vi.fn().mockResolvedValue([]) },
  ApiError: class extends Error {},
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: () => [],
}))

vi.mock('@/api/sseClient', () => ({ openStandingsStream: vi.fn(() => () => {}) }))

import { api } from '@/api/client'
import HeroStatsView from './HeroStatsView.vue'
import { useTournamentsStore } from '@/stores/tournaments'

/**
 * The all-heroes table's header clicks, which is what this file exists to pin.
 *
 * Ordering and ranks are already tested as data in `domain/heroStats.spec.ts`;
 * this mounts only to assert the join between a header and the sort it drives.
 */
function ranked(entries: [number, string, number, number][]): HeroRankRow[] {
  return entries.map(([heroId, heroName, points, rank]) => ({ rank, heroId, heroName, cost: 2000, points }))
}

const BOARD: HeroStatsBoard = {
  tournamentId: 7,
  ruleSetName: 'Standard',
  currentRound: 2,
  overall: ranked([
    [1, 'Alice', 30, 1],
    [2, 'Bigfoot', 20, 2],
    [3, 'Medusa', 10, 3],
  ]),
  categories: [
    {
      metric: 'WIN',
      label: 'Win',
      coefficient: 10,
      rows: ranked([
        [3, 'Medusa', 20, 1],
        [2, 'Bigfoot', 10, 2],
        [1, 'Alice', 0, 3],
      ]),
    },
  ],
}

const LIVE = { id: 7, name: 'Winter of Champions', status: 'LIVE' } as Tournament

async function render() {
  useTournamentsStore().tournaments = [LIVE]
  const wrapper = mount(HeroStatsView)
  await flushPromises()
  return wrapper
}

function matrixRows(wrapper: Awaited<ReturnType<typeof render>>) {
  return wrapper.findAll('#hero-matrix tbody tr').map((tr) => {
    const cells = tr.findAll('td')
    return [cells[0]!.text(), cells[1]!.find('p').text()]
  })
}

function header(wrapper: Awaited<ReturnType<typeof render>>, label: string) {
  return wrapper.findAll('#hero-matrix th').find((th) => th.text().startsWith(label))!
}

describe('HeroStatsView all-heroes table', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(api.heroStats).mockResolvedValue(BOARD)
  })

  it('opens on the overall ranking', async () => {
    const wrapper = await render()

    expect(matrixRows(wrapper)).toEqual([
      ['1', 'Alice'],
      ['2', 'Bigfoot'],
      ['3', 'Medusa'],
    ])
    expect(header(wrapper, 'Overall').attributes('aria-sort')).toBe('descending')
    expect(header(wrapper, 'Win').attributes('aria-sort')).toBe('none')
  })

  it('sorts by a criterion when its header is clicked, then flips on a second click', async () => {
    const wrapper = await render()

    await header(wrapper, 'Win').find('button').trigger('click')
    expect(matrixRows(wrapper)).toEqual([
      ['1', 'Medusa'],
      ['2', 'Bigfoot'],
      ['3', 'Alice'],
    ])
    expect(header(wrapper, 'Win').attributes('aria-sort')).toBe('descending')
    expect(header(wrapper, 'Overall').attributes('aria-sort')).toBe('none')

    await header(wrapper, 'Win').find('button').trigger('click')
    expect(matrixRows(wrapper)).toEqual([
      ['3', 'Alice'],
      ['2', 'Bigfoot'],
      ['1', 'Medusa'],
    ])
    expect(header(wrapper, 'Win').attributes('aria-sort')).toBe('ascending')
  })
})
