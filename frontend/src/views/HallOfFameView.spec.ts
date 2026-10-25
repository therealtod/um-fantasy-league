import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { HallOfFame, StandingsRow } from '@/api/types'

vi.mock('@/api/client', () => ({
  api: { hallOfFame: vi.fn() },
  ApiError: class extends Error {},
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: () => [],
}))

import { api } from '@/api/client'
import HallOfFameView from './HallOfFameView.vue'

function row(rank: number, managerId: number, handle: string, totalPoints: number): StandingsRow {
  return {
    rank,
    entryId: managerId * 10,
    managerId,
    handle,
    displayName: `${handle} Display`,
    roster: ['Medusa', 'Bigfoot'],
    spent: 5000,
    creditGrant: 10000,
    totalPoints,
    roundPoints: 0,
    breakdown: {},
  }
}

const HALL: HallOfFame = {
  titles: [
    { rank: 1, managerId: 1, handle: 'Alpha', displayName: 'Alpha Display', titles: 1, podiums: 1 },
    { rank: 2, managerId: 2, handle: 'Bravo', displayName: 'Bravo Display', titles: 0, podiums: 1 },
    { rank: 2, managerId: 3, handle: 'Charlie', displayName: 'Charlie Display', titles: 0, podiums: 1 },
  ],
  tournaments: [
    {
      tournamentId: 9,
      name: 'Autumn Finals',
      format: 'ARSENAL',
      startDate: '2026-10-01',
      ruleSetName: 'Standard',
      entrants: 0,
      podium: [],
    },
    {
      tournamentId: 1,
      name: 'Summer of Legends',
      format: 'BANQUEST',
      startDate: '2026-06-05',
      endDate: '2026-06-07',
      ruleSetName: 'Standard',
      entrants: 5,
      // A tie for second: two "2nd"s and no "3rd".
      podium: [row(1, 1, 'Alpha', 90), row(2, 2, 'Bravo', 80), row(2, 3, 'Charlie', 80)],
    },
  ],
}

async function render() {
  const wrapper = mount(HallOfFameView)
  await flushPromises()
  return wrapper
}

describe('HallOfFameView', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(api.hallOfFame).mockResolvedValue(HALL)
  })

  it('lists the titles table in the server order', async () => {
    const wrapper = await render()

    const rows = wrapper.findAll('#hall-of-fame-titles tbody tr').map((tr) => {
      const cells = tr.findAll('td')
      return [cells[0]!.text(), cells[1]!.find('p').text(), cells[2]!.text(), cells[3]!.text()]
    })
    expect(rows).toEqual([
      ['1', 'Alpha', '1', '1'],
      ['2', 'Bravo', '0', '1'],
      ['2', 'Charlie', '0', '1'],
    ])
  })

  it('renders one card per tournament, newest first, with places by rank', async () => {
    const wrapper = await render()

    const cards = wrapper.findAll('.hall-of-fame-tournament')
    expect(cards.map((c) => c.find('h3').text())).toEqual(['Autumn Finals', 'Summer of Legends'])

    expect(cards[0]!.text()).toContain('Nobody entered this tournament.')

    const places = cards[1]!.findAll('.podium-row').map((li) => li.find('span').text())
    expect(places).toEqual(['Champion', '2nd', '2nd'])
    expect(cards[1]!.text()).toContain('05 Jun 2026 – 07 Jun 2026')
    expect(cards[1]!.text()).toContain('90.0')
  })

  it('says so when no tournament has finished', async () => {
    vi.mocked(api.hallOfFame).mockResolvedValue({ titles: [], tournaments: [] })
    const wrapper = await render()

    expect(wrapper.find('#hall-of-fame-empty').exists()).toBe(true)
    expect(wrapper.find('#hall-of-fame-titles').exists()).toBe(false)
  })
})
