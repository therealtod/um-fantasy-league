import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { MatchImportPreviewDto } from '@/api/types'

const importMatch = vi.fn()
const addMapToPool = vi.fn()
const violationMessagesMock = vi.fn((_e: unknown) => [] as string[])

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      importMatch: (...args: unknown[]) => importMatch(...args),
      addMapToPool: (...args: unknown[]) => addMapToPool(...args),
    },
  },
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: (e: unknown) => violationMessagesMock(e),
}))

const url = 'https://www.tabletopleague.com/o/umleague/summer-of-legends-2026/matches/abcdef12'

const resolvedPreview: MatchImportPreviewDto = {
  sourceUrl: url,
  roundName: 'The Wayward Sisters',
  seriesFormat: 'BO3',
  playedAt: '2026-08-17T20:00:00Z',
  playedAtRaw: '17 Aug 2026, 22:00 CEST',
  participants: [
    { playerLabel: 'mystic_owl', draftedHeroIds: [10] },
    { playerLabel: 'immortal', draftedHeroIds: [11] },
  ],
  games: [
    {
      gameNumber: 1,
      mapId: 5,
      mapName: 'Technodrome',
      participants: [
        { heroId: 10, heroName: 'Tomoe Gozen', healthRemaining: 5, isWinner: true },
        { heroId: 11, heroName: 'Wyatt Earp', healthRemaining: 0, isWinner: false },
      ],
    },
  ],
  bans: [],
  unresolved: [],
}

const unresolvedPreview: MatchImportPreviewDto = {
  ...resolvedPreview,
  games: [{ ...resolvedPreview.games[0]!, mapId: undefined }],
  unresolved: [
    {
      kind: 'MAP',
      sourceName: 'Technodrome',
      reason: 'MAP_NOT_IN_POOL',
      mapId: 5,
      message: '"Technodrome" is not in this tournament\'s board pool, so a game cannot be recorded on it.',
    },
  ],
}

async function mountPanel() {
  const MatchImportPanel = (await import('./MatchImportPanel.vue')).default
  const wrapper = mount(MatchImportPanel, { props: { tournamentId: 7 } })
  await flushPromises()
  return wrapper
}

async function runImport(wrapper: Awaited<ReturnType<typeof mountPanel>>) {
  await wrapper.find('#import-source-url').setValue(url)
  await wrapper.find('button.btn-primary').trigger('click')
  await flushPromises()
}

beforeEach(() => {
  vi.clearAllMocks()
  violationMessagesMock.mockReturnValue([])
})

describe('MatchImportPanel', () => {
  it('will not import an empty url', async () => {
    const wrapper = await mountPanel()
    expect(wrapper.find('button.btn-primary').attributes('disabled')).toBeDefined()
    expect(importMatch).not.toHaveBeenCalled()
  })

  it('sends the trimmed url to the tournament being imported into', async () => {
    importMatch.mockResolvedValue(resolvedPreview)
    const wrapper = await mountPanel()

    await wrapper.find('#import-source-url').setValue(`  ${url}  `)
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(importMatch).toHaveBeenCalledWith(7, url)
  })

  it('shows the source round name and format as context', async () => {
    importMatch.mockResolvedValue(resolvedPreview)
    const wrapper = await mountPanel()
    await runImport(wrapper)

    expect(wrapper.text()).toContain('The Wayward Sisters')
    expect(wrapper.text()).toContain('BO3')
    expect(wrapper.text()).toContain('mystic_owl')
  })

  it('emits the preview for review once everything resolves', async () => {
    importMatch.mockResolvedValue(resolvedPreview)
    const wrapper = await mountPanel()
    await runImport(wrapper)

    const reviewButton = wrapper.findAll('button.btn-primary').at(-1)!
    expect(reviewButton.attributes('disabled')).toBeUndefined()
    await reviewButton.trigger('click')

    expect(wrapper.emitted('review')?.[0]).toEqual([resolvedPreview])
  })

  /** The whole point of the review step: an unresolved name cannot be recorded. */
  it('blocks review while a name is unresolved', async () => {
    importMatch.mockResolvedValue(unresolvedPreview)
    const wrapper = await mountPanel()
    await runImport(wrapper)

    expect(wrapper.text()).toContain('Technodrome')
    expect(wrapper.text()).toContain("couldn't be matched")
    const reviewButton = wrapper.findAll('button.btn-primary').at(-1)!
    expect(reviewButton.attributes('disabled')).toBeDefined()
    expect(wrapper.emitted('review')).toBeUndefined()
  })

  /**
   * The admin clicks to widen the board pool — the importer never does it on its
   * own. Afterwards the match is re-read so the preview reflects the new pool.
   */
  it('adds an out-of-pool board on request, then re-imports', async () => {
    importMatch.mockResolvedValueOnce(unresolvedPreview).mockResolvedValueOnce(resolvedPreview)
    addMapToPool.mockResolvedValue({ id: 5, name: 'Technodrome' })
    const wrapper = await mountPanel()
    await runImport(wrapper)

    const addButton = wrapper.findAll('button.btn-ghost').find((b) => b.text().includes('Add to board pool'))!
    await addButton.trigger('click')
    await flushPromises()

    expect(addMapToPool).toHaveBeenCalledWith(7, 5)
    expect(importMatch).toHaveBeenCalledTimes(2)
    expect(wrapper.findAll('button.btn-primary').at(-1)!.attributes('disabled')).toBeUndefined()
  })

  it('offers no pool shortcut for a hero the catalogue does not have', async () => {
    importMatch.mockResolvedValue({
      ...resolvedPreview,
      unresolved: [
        {
          kind: 'HERO',
          sourceName: 'Nonexistent Hero',
          reason: 'UNKNOWN_HERO',
          message: 'No hero named "Nonexistent Hero" exists.',
        },
      ],
    })
    const wrapper = await mountPanel()
    await runImport(wrapper)

    expect(wrapper.text()).toContain('Nonexistent Hero')
    expect(wrapper.findAll('button.btn-ghost').some((b) => b.text().includes('Add to board pool'))).toBe(false)
    expect(wrapper.text()).toContain('Add the missing entries under Heroes or Maps')
  })

  it('blocks recording a url already imported, and offers to correct that match instead', async () => {
    importMatch.mockResolvedValue({ ...resolvedPreview, alreadyImportedMatchId: 42 })
    const wrapper = await mountPanel()
    await runImport(wrapper)

    expect(wrapper.text()).toContain('Already imported')
    expect(wrapper.text()).toContain('#42')
    // Recording a second copy would double-count every point the match scores,
    // and the server refuses it — so the panel refuses to offer it.
    expect(wrapper.findAll('button.btn-primary').at(-1)!.attributes('disabled')).toBeDefined()

    // The way out is the existing match, not a second one.
    const correct = wrapper.findAll('button').find((b) => b.text().includes('Open match #42'))
    expect(correct).toBeDefined()
    await correct!.trigger('click')
    expect(wrapper.emitted('correctExisting')).toEqual([[42]])
    expect(wrapper.emitted('review')).toBeUndefined()
  })

  /** A scraper that isn't running has to read as something the admin can act on. */
  it('surfaces a failed import as an error instead of a blank panel', async () => {
    importMatch.mockRejectedValue(new Error('The match scraper at http://localhost:3000 is not reachable.'))
    const wrapper = await mountPanel()
    await runImport(wrapper)

    expect(wrapper.text()).toContain('not reachable')
    expect(wrapper.emitted('review')).toBeUndefined()
  })

  it('flags a timestamp the importer could not read', async () => {
    importMatch.mockResolvedValue({
      ...resolvedPreview,
      playedAt: undefined,
      playedAtRaw: 'sometime on Tuesday',
    })
    const wrapper = await mountPanel()
    await runImport(wrapper)

    expect(wrapper.text()).toContain('sometime on Tuesday')
    expect(wrapper.text()).toContain("timestamp couldn't be read")
  })
})
