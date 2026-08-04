import { flushPromises, mount } from '@vue/test-utils'
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
}))

const matches: MatchResultDto[] = [
  {
    matchId: 1,
    tournamentId: 1,
    round: 1,
    mapId: 5,
    mapName: 'Center Square',
    playedAt: '2026-08-01T12:00:00Z',
    participants: [
      { participantId: 1, heroId: 10, heroName: 'Sherlock Holmes', healthRemaining: 8, isWinner: true },
      { participantId: 2, heroId: 11, heroName: 'Dracula', healthRemaining: 0, isWinner: false },
    ],
    bans: [],
  },
  {
    matchId: 2,
    tournamentId: 1,
    round: 2,
    mapId: 6,
    mapName: 'Baskerville Manor',
    playedAt: '2026-08-02T12:00:00Z',
    participants: [
      { participantId: 3, heroId: 12, heroName: 'Medusa', healthRemaining: 5, isWinner: false },
      { participantId: 4, heroId: 13, heroName: 'King Arthur', healthRemaining: 5, isWinner: false },
    ],
    bans: [{ heroId: 14, heroName: 'Achilles' }],
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
  vi.spyOn(window, 'confirm').mockReturnValue(true)
  listMatches.mockResolvedValue(matches)
  deleteMatch.mockResolvedValue(undefined)
})

describe('MatchListAdmin', () => {
  it('loads the tournament\'s matches on mount', async () => {
    const wrapper = await mountList()

    expect(listMatches).toHaveBeenCalledWith(1, undefined)
    expect(wrapper.text()).toContain('Center Square')
    expect(wrapper.text()).toContain('Baskerville Manor')
  })

  it('offers every round the loaded matches contain, in order', async () => {
    const wrapper = await mountList()

    const options = wrapper.findAll('#round-filter option').map((o) => o.text().trim())
    expect(options).toEqual(['All Rounds', 'Round 1', 'Round 2'])
  })

  it('filters the visible rows to the selected round and re-requests it from the server', async () => {
    const wrapper = await mountList()

    await wrapper.find('#round-filter').setValue('1')
    await flushPromises()

    expect(listMatches).toHaveBeenLastCalledWith(1, 1)
    expect(wrapper.text()).toContain('Center Square')
    expect(wrapper.text()).not.toContain('Baskerville Manor')
  })

  it('shows the winning hero, falling back to the player label when recorded', async () => {
    const withPlayer: MatchResultDto = {
      ...matches[0]!,
      participants: [
        { ...matches[0]!.participants[0]!, playerLabel: 'Alice' },
        matches[0]!.participants[1]!,
      ],
    }
    listMatches.mockResolvedValue([withPlayer, matches[1]!])
    const wrapper = await mountList()

    expect(wrapper.text()).toContain('Sherlock Holmes (Alice)')
    // A match with no winner reports a draw rather than a blank cell.
    expect(wrapper.text()).toContain('Draw')
  })

  it('emits create when asked to record a new match', async () => {
    const wrapper = await mountList()

    await wrapper.find('button.btn-primary').trigger('click')

    expect(wrapper.emitted('create')).toHaveLength(1)
  })

  it('emits edit with the match id', async () => {
    const wrapper = await mountList()

    await wrapper.findAll('button').find((b) => b.text() === 'Edit')!.trigger('click')

    expect(wrapper.emitted('edit')).toEqual([[1]])
  })

  it('asks for confirmation before deleting, and reloads the list once confirmed', async () => {
    const wrapper = await mountList()
    listMatches.mockResolvedValue([matches[1]!])

    await wrapper.findAll('button').find((b) => b.text() === 'Delete')!.trigger('click')
    await flushPromises()

    expect(window.confirm).toHaveBeenCalled()
    expect(deleteMatch).toHaveBeenCalledWith(1, 1)
    expect(listMatches).toHaveBeenCalledTimes(2)
    expect(wrapper.text()).not.toContain('Center Square')
  })

  it('leaves the list untouched when the delete confirmation is declined', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(false)
    const wrapper = await mountList()

    await wrapper.findAll('button').find((b) => b.text() === 'Delete')!.trigger('click')
    await flushPromises()

    expect(deleteMatch).not.toHaveBeenCalled()
    expect(listMatches).toHaveBeenCalledTimes(1)
  })

  it('surfaces a failed load as an error rather than an empty table', async () => {
    listMatches.mockRejectedValue(new Error('Tournament not found'))
    const wrapper = await mountList()

    expect(wrapper.text()).toContain('Tournament not found')
  })

  it('surfaces a rejected delete as an error', async () => {
    deleteMatch.mockRejectedValue(new Error('Match already deleted'))
    const wrapper = await mountList()

    await wrapper.findAll('button').find((b) => b.text() === 'Delete')!.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Match already deleted')
  })
})
