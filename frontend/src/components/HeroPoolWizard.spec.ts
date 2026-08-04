import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Hero, HeroAdminDto } from '@/api/types'

const listHeroes = vi.fn()
const listHeroPool = vi.fn()
const removeHeroFromPool = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      listHeroes: (...args: unknown[]) => listHeroes(...args),
      listHeroPool: (...args: unknown[]) => listHeroPool(...args),
      setHeroCost: vi.fn(),
      removeHeroFromPool: (...args: unknown[]) => removeHeroFromPool(...args),
    },
  },
}))

vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({ tournaments: [{ id: 1, name: 'Winter Open' }] }),
}))

const heroes: HeroAdminDto[] = [
  { id: 10, name: 'Sherlock Holmes', imageUrl: null },
  { id: 11, name: 'Dracula', imageUrl: null },
]
const heroPool: Hero[] = [{ id: 10, name: 'Sherlock Holmes', imageUrl: null, cost: 500 }]

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

beforeEach(() => {
  vi.clearAllMocks()
  listHeroes.mockResolvedValue(heroes)
  listHeroPool.mockResolvedValue(heroPool)
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
