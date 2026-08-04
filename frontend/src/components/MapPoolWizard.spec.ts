import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { MapAdminDto } from '@/api/types'

const listMaps = vi.fn()
const listMapPool = vi.fn()
const removeMapFromPool = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      listMaps: (...args: unknown[]) => listMaps(...args),
      listMapPool: (...args: unknown[]) => listMapPool(...args),
      addMapToPool: vi.fn(),
      removeMapFromPool: (...args: unknown[]) => removeMapFromPool(...args),
    },
  },
}))

vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({ tournaments: [{ id: 1, name: 'Winter Open' }] }),
}))

const maps: MapAdminDto[] = [
  { id: 5, name: 'Center Square' },
  { id: 6, name: 'Marmoreal' },
]
const mapPool: MapAdminDto[] = [{ id: 5, name: 'Center Square' }]

async function mountWizardWithTournament() {
  const MapPoolWizard = (await import('./MapPoolWizard.vue')).default
  const wrapper = mount(MapPoolWizard)
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
  listMaps.mockResolvedValue(maps)
  listMapPool.mockResolvedValue(mapPool)
  removeMapFromPool.mockResolvedValue(undefined)
})

describe('MapPoolWizard removal', () => {
  it('asks for confirmation before removing', async () => {
    const wrapper = await mountWizardWithTournament()

    await removeButton(wrapper)!.trigger('click')

    expect(removeMapFromPool).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Remove Map from Pool')
    expect(wrapper.text()).toContain('Center Square')
  })

  it('removes the map and reloads the pool once confirmed', async () => {
    const wrapper = await mountWizardWithTournament()
    listMapPool.mockResolvedValue([])

    await removeButton(wrapper)!.trigger('click')
    await wrapper.findAll('button').find((b) => b.text() === 'Remove from Pool')!.trigger('click')
    await flushPromises()

    expect(removeMapFromPool).toHaveBeenCalledWith(1, 5)
    expect(listMapPool).toHaveBeenCalledTimes(2)
    expect(wrapper.text()).toContain("No maps in this tournament's pool yet")
  })

  it('surfaces the 409 a board with a recorded match comes back with', async () => {
    const wrapper = await mountWizardWithTournament()
    removeMapFromPool.mockRejectedValue(
      new Error('Map is used by a recorded match in this tournament'),
    )

    await removeButton(wrapper)!.trigger('click')
    await wrapper.findAll('button').find((b) => b.text() === 'Remove from Pool')!.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Map is used by a recorded match')
    // The confirmation stays open on failure so the admin can retry or back out.
    expect(wrapper.text()).toContain('Remove Map from Pool')
  })
})
