import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { MapAdminDto } from '@/api/types'

const listMaps = vi.fn()
const listMapPool = vi.fn()
const addMapsToPool = vi.fn()
const removeMapFromPool = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      listMaps: (...args: unknown[]) => listMaps(...args),
      listMapPool: (...args: unknown[]) => listMapPool(...args),
      addMapToPool: vi.fn(),
      addMapsToPool: (...args: unknown[]) => addMapsToPool(...args),
      removeMapFromPool: (...args: unknown[]) => removeMapFromPool(...args),
    },
  },
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
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

function mapCheckbox(wrapper: Awaited<ReturnType<typeof mountWizardWithTournament>>, mapId: number) {
  return wrapper.find(`input[type="checkbox"][value="${mapId}"]`)
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

describe('MapPoolWizard batch add', () => {
  it('checks maps locally without calling the API', async () => {
    const wrapper = await mountWizardWithTournament()

    await mapCheckbox(wrapper, 6).setValue(true)

    expect(addMapsToPool).not.toHaveBeenCalled()
    expect(wrapper.findAll('button').find((b) => b.text() === 'Add 1 Map to Pool')).toBeTruthy()
  })

  it('submits every checked map in one batch request', async () => {
    const wrapper = await mountWizardWithTournament()
    addMapsToPool.mockResolvedValue([])

    await mapCheckbox(wrapper, 6).setValue(true)
    await wrapper.findAll('button').find((b) => b.text() === 'Add 1 Map to Pool')!.trigger('click')
    await flushPromises()

    expect(addMapsToPool).toHaveBeenCalledTimes(1)
    expect(addMapsToPool).toHaveBeenCalledWith(1, [6])
    expect(listMapPool).toHaveBeenCalledTimes(2)
  })

  it('keeps the selection if the batch submit fails', async () => {
    const wrapper = await mountWizardWithTournament()
    addMapsToPool.mockRejectedValue(new Error('Map 6 is already recorded'))

    await mapCheckbox(wrapper, 6).setValue(true)
    await wrapper.findAll('button').find((b) => b.text() === 'Add 1 Map to Pool')!.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Map 6 is already recorded')
    expect((mapCheckbox(wrapper, 6).element as HTMLInputElement).checked).toBe(true)
  })
})
