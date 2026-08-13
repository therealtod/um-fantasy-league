import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { MapAdminDto } from '@/api/types'

const listMaps = vi.fn()
const createMap = vi.fn()
const updateMap = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      listMaps: (...args: unknown[]) => listMaps(...args),
      createMap: (...args: unknown[]) => createMap(...args),
      updateMap: (...args: unknown[]) => updateMap(...args),
    },
  },
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
}))

const maps: MapAdminDto[] = [
  { id: 5, name: 'Center Square' },
  { id: 6, name: 'Baskerville Manor' },
]

async function mountWizard() {
  const MapManagementWizard = (await import('./MapManagementWizard.vue')).default
  const wrapper = mount(MapManagementWizard)
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  listMaps.mockResolvedValue(maps)
})

describe('MapManagementWizard', () => {
  it('loads and lists every map on mount', async () => {
    const wrapper = await mountWizard()

    expect(listMaps).toHaveBeenCalled()
    expect(wrapper.text()).toContain('Center Square')
    expect(wrapper.text()).toContain('Baskerville Manor')
  })

  it('creates a map from the form and appends it to the list', async () => {
    createMap.mockResolvedValue({ id: 7, name: 'Sherwood Forest' })
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click') // + Create Map
    await wrapper.find('#map-name').setValue('Sherwood Forest')
    await wrapper.find('button.btn-primary').trigger('click') // Save Map
    await flushPromises()

    expect(createMap).toHaveBeenCalledWith({ name: 'Sherwood Forest' })
    expect(wrapper.text()).toContain('Sherwood Forest')
    expect(wrapper.find('#map-name').exists()).toBe(false)
  })

  it('refuses to save a blank name', async () => {
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click')
    await wrapper.find('button.btn-primary').trigger('click')

    expect(createMap).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Map name is required')
  })

  it('edits an existing map in place rather than appending a duplicate', async () => {
    updateMap.mockResolvedValue({ id: 5, name: 'Center Plaza' })
    const wrapper = await mountWizard()

    await wrapper.findAll('button').find((b) => b.text() === 'Edit')!.trigger('click')
    expect((wrapper.find('#map-name').element as HTMLInputElement).value).toBe('Center Square')

    await wrapper.find('#map-name').setValue('Center Plaza')
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(updateMap).toHaveBeenCalledWith(5, { name: 'Center Plaza' })
    expect(wrapper.text()).toContain('Center Plaza')
    expect(wrapper.text()).not.toContain('Center Square')
    expect(createMap).not.toHaveBeenCalled()
  })

  it('surfaces a failed save as an error', async () => {
    createMap.mockRejectedValue(new Error('Map name already exists'))
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click')
    await wrapper.find('#map-name').setValue('Baskerville Manor')
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Map name already exists')
  })

  it('cancelling the form discards the draft and returns to the list', async () => {
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click')
    await wrapper.find('#map-name').setValue('Draft Map')
    await wrapper.find('button.btn-ghost').trigger('click')

    expect(wrapper.find('#map-name').exists()).toBe(false)
    expect(createMap).not.toHaveBeenCalled()
  })
})
