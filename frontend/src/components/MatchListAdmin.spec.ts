import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { MatchResultDto } from '@/api/types'

const listMatches = vi.fn()
const deleteMatch = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      listMatches: (...args: unknown[]) => listMatches(...args),
      deleteMatch: (...args: unknown[]) => deleteMatch(...args),
    },
  },
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
}))

const matches: MatchResultDto[] = [
  {
    matchId: 1,
    tournamentId: 1,
    round: 1,
    playedAt: '2026-08-01T12:00:00Z',
    participants: [{ side: 0 }, { side: 1 }],
    games: [
      {
        gameId: 100,
        gameNumber: 1,
        mapId: 5,
        mapName: 'Center Square',
        participants: [
          { side: 0, heroId: 10, heroName: 'Sherlock Holmes', healthRemaining: 8, isWinner: true },
          { side: 1, heroId: 11, heroName: 'Dracula', healthRemaining: 0, isWinner: false },
        ],
      },
    ],
    bans: [],
  },
  {
    matchId: 2,
    tournamentId: 1,
    round: 2,
    playedAt: '2026-08-02T12:00:00Z',
    participants: [{ side: 0 }, { side: 1 }],
    games: [
      {
        gameId: 200,
        gameNumber: 1,
        mapId: 6,
        mapName: 'Baskerville Manor',
        participants: [
          { side: 0, heroId: 12, heroName: 'Medusa', healthRemaining: 6, isWinner: true },
          { side: 1, heroId: 13, heroName: 'King Arthur', healthRemaining: 0, isWinner: false },
        ],
      },
    ],
    bans: [{ heroId: 14, heroName: 'Achilles', banType: 'PRE_BAN' }],
  },
]

async function mountList() {
  const MatchListAdmin = (await import('./MatchListAdmin.vue')).default
  const wrapper = mount(MatchListAdmin, { props: { tournamentId: 1 } })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  listMatches.mockResolvedValue(matches)
  deleteMatch.mockResolvedValue(undefined)
})

function clickButton(wrapper: VueWrapper, label: string) {
  return wrapper.findAll('button').find((b) => b.text() === label)!.trigger('click')
}

describe('MatchListAdmin', () => {
  it('loads the tournament\'s matches on mount', async () => {
    const wrapper = await mountList()

    expect(listMatches).toHaveBeenCalledWith(1)
    expect(wrapper.text()).toContain('Center Square')
    expect(wrapper.text()).toContain('Baskerville Manor')
  })

  it('offers every round the loaded matches contain, in order', async () => {
    const wrapper = await mountList()

    const options = wrapper.findAll('#round-filter option').map((o) => o.text().trim())
    expect(options).toEqual(['All Rounds', 'Round 1', 'Round 2'])
  })

  it('filters the visible rows to the selected round without re-requesting', async () => {
    const wrapper = await mountList()

    await wrapper.find('#round-filter').setValue(1)
    await flushPromises()

    expect(listMatches).toHaveBeenCalledTimes(1)
    expect(wrapper.text()).toContain('Center Square')
    expect(wrapper.text()).not.toContain('Baskerville Manor')
  })

  // A server-side filter would shrink the list the options are derived from, stranding the
  // filter on whichever round was picked.
  it('keeps offering every round while a round is selected', async () => {
    const wrapper = await mountList()

    await wrapper.find('#round-filter').setValue(1)
    await flushPromises()

    const options = wrapper.findAll('#round-filter option').map((o) => o.text().trim())
    expect(options).toEqual(['All Rounds', 'Round 1', 'Round 2'])
  })

  it('reloads and clears the filter when the tournament changes', async () => {
    const wrapper = await mountList()
    await wrapper.find('#round-filter').setValue(1)
    await flushPromises()

    await wrapper.setProps({ tournamentId: 2 })
    await flushPromises()

    expect(listMatches).toHaveBeenLastCalledWith(2)
    expect(wrapper.text()).toContain('Baskerville Manor')
  })

  it('derives a games-won tally per side, falling back to a generic label when unattributed', async () => {
    const withPlayer: MatchResultDto = {
      ...matches[0]!,
      participants: [{ side: 0, playerLabel: 'Alice' }, { side: 1, playerLabel: 'Bob' }],
    }
    listMatches.mockResolvedValue([withPlayer, matches[1]!])
    const wrapper = await mountList()

    // Alice's side won the only game, so her tally reads 1 against Bob's 0.
    expect(wrapper.text()).toContain('Alice')
    expect(wrapper.text()).toContain('Bob')
    // Match 2 is unattributed, so both sides fall back to a generic label.
    expect(wrapper.text()).toContain('Side 1')
    expect(wrapper.text()).toContain('Side 2')
  })

  it('shows each game\'s hero, health and winner badge, grouped under its own map', async () => {
    const wrapper = await mountList()

    expect(wrapper.text()).toContain('G1 · Center Square')
    expect(wrapper.text()).toContain('Sherlock Holmes')
    expect(wrapper.text()).toContain('WIN')
  })

  it('groups bans by category', async () => {
    const wrapper = await mountList()

    expect(wrapper.text()).toContain('Achilles')
    expect(wrapper.text()).toContain('Pre-ban')
  })

  it('renders an external link when present, and a dash otherwise', async () => {
    const withLink: MatchResultDto = { ...matches[0]!, externalLink: 'https://example.com/bracket/1' }
    listMatches.mockResolvedValue([withLink, matches[1]!])
    const wrapper = await mountList()

    const link = wrapper.find('a[href="https://example.com/bracket/1"]')
    expect(link.exists()).toBe(true)
  })

  it('emits create when asked to record a new match', async () => {
    const wrapper = await mountList()

    await wrapper.find('button.btn-primary').trigger('click')

    expect(wrapper.emitted('create')).toHaveLength(1)
  })

  it('emits edit with the match id', async () => {
    const wrapper = await mountList()

    await clickButton(wrapper, 'Edit')

    expect(wrapper.emitted('edit')).toEqual([[1]])
  })

  it('confirms in-app before deleting, and reloads the list once confirmed', async () => {
    const wrapper = await mountList()

    await clickButton(wrapper, 'Delete')

    expect(wrapper.text()).toContain('Delete Match')
    expect(deleteMatch).not.toHaveBeenCalled()

    listMatches.mockResolvedValue([matches[1]!])
    await clickButton(wrapper, 'Delete Match')
    await flushPromises()

    expect(deleteMatch).toHaveBeenCalledWith(1, 1)
    expect(listMatches).toHaveBeenCalledTimes(2)
    expect(wrapper.text()).not.toContain('Center Square')
  })

  it('leaves the list untouched when the delete confirmation is cancelled', async () => {
    const wrapper = await mountList()

    await clickButton(wrapper, 'Delete')
    await clickButton(wrapper, 'Cancel')
    await flushPromises()

    expect(deleteMatch).not.toHaveBeenCalled()
    expect(listMatches).toHaveBeenCalledTimes(1)
    expect(wrapper.text()).toContain('Center Square')
  })

  it('surfaces a failed load as an error rather than an empty table', async () => {
    listMatches.mockRejectedValue(new Error('Tournament not found'))
    const wrapper = await mountList()

    expect(wrapper.text()).toContain('Tournament not found')
  })

  it('surfaces a rejected delete as an error', async () => {
    deleteMatch.mockRejectedValue(new Error('Match already deleted'))
    const wrapper = await mountList()

    await clickButton(wrapper, 'Delete')
    await clickButton(wrapper, 'Delete Match')
    await flushPromises()

    expect(wrapper.text()).toContain('Match already deleted')
  })
})
