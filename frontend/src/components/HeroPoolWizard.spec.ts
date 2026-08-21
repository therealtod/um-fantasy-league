import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Hero, HeroAdminDto } from '@/api/types'

const listHeroes = vi.fn()
const listHeroPool = vi.fn()
const addHeroesToPool = vi.fn()
const setHeroCost = vi.fn()
const removeHeroFromPool = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      listHeroes: (...args: unknown[]) => listHeroes(...args),
      listHeroPool: (...args: unknown[]) => listHeroPool(...args),
      addHeroesToPool: (...args: unknown[]) => addHeroesToPool(...args),
      setHeroCost: (...args: unknown[]) => setHeroCost(...args),
      removeHeroFromPool: (...args: unknown[]) => removeHeroFromPool(...args),
    },
  },
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: () => [],
}))

vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({ tournaments: [{ id: 1, name: 'Winter Open' }] }),
}))

const heroes: HeroAdminDto[] = [
  { id: 10, name: 'Sherlock Holmes', imageUrl: null },
  { id: 11, name: 'Dracula', imageUrl: null },
]
// A factory, not a shared array: a cost edit now mutates the returned hero
// in place (see HeroPoolWizard.vue), so a fixture reused by reference would
// leak an edit from one test into the next.
const makeHeroPool = (): Hero[] => [{ id: 10, name: 'Sherlock Holmes', imageUrl: null, cost: 500 }]

async function mountWizardWithTournament() {
  const HeroPoolWizard = (await import('./HeroPoolWizard.vue')).default
  const wrapper = mount(HeroPoolWizard)
  await flushPromises()
  await wrapper.find('#tournament-select').setValue(1)
  await flushPromises()
  return wrapper
}

function removeButton(wrapper: Awaited<ReturnType<typeof mountWizardWithTournament>>) {
  return wrapper.findAll('button').find((b) => b.text() === 'Remove')
}

async function openAddForm(wrapper: Awaited<ReturnType<typeof mountWizardWithTournament>>) {
  await wrapper.findAll('button').find((b) => b.text() === '+ Add Hero to Pool')!.trigger('click')
}

async function stageHero(
  wrapper: Awaited<ReturnType<typeof mountWizardWithTournament>>,
  heroId: number,
  cost: number,
) {
  await wrapper.find('#hero-select').setValue(heroId)
  await wrapper.find('#hero-cost-input').setValue(cost)
  await wrapper.findAll('button').find((b) => b.text() === '+ Stage Hero')!.trigger('click')
}

beforeEach(() => {
  vi.clearAllMocks()
  listHeroes.mockResolvedValue(heroes)
  listHeroPool.mockImplementation(() => Promise.resolve(makeHeroPool()))
  removeHeroFromPool.mockResolvedValue(undefined)
})

describe('HeroPoolWizard removal', () => {
  it('asks for confirmation before removing, and explains the re-pricing side effect', async () => {
    const wrapper = await mountWizardWithTournament()

    await removeButton(wrapper)!.trigger('click')

    expect(removeHeroFromPool).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Remove Hero from Pool')
    expect(wrapper.text()).toContain('Sherlock Holmes')
    expect(wrapper.text()).toContain('re-price it to 0 credits')
  })

  it('removes the hero and reloads the pool once confirmed', async () => {
    const wrapper = await mountWizardWithTournament()
    listHeroPool.mockResolvedValue([])

    await removeButton(wrapper)!.trigger('click')
    await wrapper.findAll('button').find((b) => b.text() === 'Remove from Pool')!.trigger('click')
    await flushPromises()

    expect(removeHeroFromPool).toHaveBeenCalledWith(1, 10)
    // Pool re-read after the write rather than patched client-side.
    expect(listHeroPool).toHaveBeenCalledTimes(2)
    expect(wrapper.text()).toContain("No heroes in this tournament's pool yet")
  })

  it('cancelling leaves the pool untouched', async () => {
    const wrapper = await mountWizardWithTournament()

    await removeButton(wrapper)!.trigger('click')
    await wrapper.findAll('button').find((b) => b.text() === 'Cancel')!.trigger('click')

    expect(removeHeroFromPool).not.toHaveBeenCalled()
    expect(wrapper.text()).not.toContain('Remove Hero from Pool')
  })

  it('surfaces a rejected removal as an error instead of dropping it', async () => {
    const wrapper = await mountWizardWithTournament()
    removeHeroFromPool.mockRejectedValue(new Error('Hero pool entry not found'))

    await removeButton(wrapper)!.trigger('click')
    await wrapper.findAll('button').find((b) => b.text() === 'Remove from Pool')!.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Hero pool entry not found')
  })
})

describe('HeroPoolWizard cost edit', () => {
  it('patches the row from the response instead of reloading the whole pool', async () => {
    const wrapper = await mountWizardWithTournament()
    setHeroCost.mockResolvedValue({ id: 10, name: 'Sherlock Holmes', imageUrl: null, cost: 900 })

    // setValue() already dispatches both input and change.
    await wrapper.find('#cost-10').setValue(900)
    await flushPromises()

    expect(setHeroCost).toHaveBeenCalledWith(1, 10, 900)
    // Only the initial load — the single-field edit doesn't refetch the pool.
    expect(listHeroPool).toHaveBeenCalledTimes(1)
    expect((wrapper.find('#cost-10').element as HTMLInputElement).value).toBe('900')
  })

  it('reverts an invalid draft without calling the API', async () => {
    const wrapper = await mountWizardWithTournament()

    await wrapper.find('#cost-10').setValue(0)
    await flushPromises()

    expect(setHeroCost).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Cost must be a number of at least 1 credit')
    expect((wrapper.find('#cost-10').element as HTMLInputElement).value).toBe('500')
  })

  it('reverts to the last known cost when the server rejects the edit', async () => {
    const wrapper = await mountWizardWithTournament()
    setHeroCost.mockRejectedValue(new Error('cost must be positive'))

    await wrapper.find('#cost-10').setValue(900)
    await flushPromises()

    expect(wrapper.text()).toContain('cost must be positive')
    expect((wrapper.find('#cost-10').element as HTMLInputElement).value).toBe('500')
    // The rejection isn't papered over by a reload either.
    expect(listHeroPool).toHaveBeenCalledTimes(1)
  })
})

describe('HeroPoolWizard batch add', () => {
  it('stages heroes locally without calling the API', async () => {
    const wrapper = await mountWizardWithTournament()
    await openAddForm(wrapper)

    await stageHero(wrapper, 11, 750)

    expect(addHeroesToPool).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Staged for this batch (1)')
    expect(wrapper.text()).toContain('Dracula')
  })

  it('submits every staged hero in one batch request', async () => {
    const wrapper = await mountWizardWithTournament()
    addHeroesToPool.mockResolvedValue([])
    await openAddForm(wrapper)
    await stageHero(wrapper, 11, 750)

    await wrapper.findAll('button').find((b) => b.text() === 'Add 1 Hero to Pool')!.trigger('click')
    await flushPromises()

    expect(addHeroesToPool).toHaveBeenCalledTimes(1)
    expect(addHeroesToPool).toHaveBeenCalledWith(1, [{ heroId: 11, cost: 750 }])
    // Pool re-read after the write rather than patched client-side.
    expect(listHeroPool).toHaveBeenCalledTimes(2)
    expect(wrapper.text()).not.toContain('Staged for this batch')
  })

  it('keeps the staged heroes if the batch submit fails', async () => {
    const wrapper = await mountWizardWithTournament()
    addHeroesToPool.mockRejectedValue(new Error('cost must be positive'))
    await openAddForm(wrapper)
    await stageHero(wrapper, 11, 750)

    await wrapper.findAll('button').find((b) => b.text() === 'Add 1 Hero to Pool')!.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('cost must be positive')
    expect(wrapper.text()).toContain('Staged for this batch (1)')
  })
})
