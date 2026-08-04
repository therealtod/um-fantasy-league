import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { HeroAdminDto } from '@/api/types'

const listHeroes = vi.fn()
const createHero = vi.fn()
const updateHero = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      listHeroes: (...args: unknown[]) => listHeroes(...args),
      createHero: (...args: unknown[]) => createHero(...args),
      updateHero: (...args: unknown[]) => updateHero(...args),
    },
  },
}))

const heroes: HeroAdminDto[] = [
  { id: 10, name: 'Sherlock Holmes', imageUrl: null },
  { id: 11, name: 'Dracula', imageUrl: 'https://example.com/dracula.png' },
]

async function mountWizard() {
  const HeroManagementWizard = (await import('./HeroManagementWizard.vue')).default
  const wrapper = mount(HeroManagementWizard)
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  listHeroes.mockResolvedValue(heroes)
})

describe('HeroManagementWizard', () => {
  it('loads and lists every hero on mount', async () => {
    const wrapper = await mountWizard()

    expect(listHeroes).toHaveBeenCalled()
    expect(wrapper.text()).toContain('Sherlock Holmes')
    expect(wrapper.text()).toContain('Dracula')
    expect(wrapper.text()).toContain('No image')
  })

  it('creates a hero from the form and appends it to the list', async () => {
    createHero.mockResolvedValue({ id: 12, name: 'Medusa', imageUrl: null })
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click') // + Create Hero
    await wrapper.find('#hero-name').setValue('Medusa')
    await wrapper.find('button.btn-primary').trigger('click') // Save Hero
    await flushPromises()

    expect(createHero).toHaveBeenCalledWith({ name: 'Medusa', imageUrl: null })
    expect(wrapper.text()).toContain('Medusa')
    // The form closes back to the list once saved.
    expect(wrapper.find('#hero-name').exists()).toBe(false)
  })

  it('refuses to save a blank name', async () => {
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click')
    await wrapper.find('button.btn-primary').trigger('click')

    expect(createHero).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Hero name is required')
  })

  it('edits an existing hero in place rather than appending a duplicate', async () => {
    updateHero.mockResolvedValue({ id: 10, name: 'Enola Holmes', imageUrl: null })
    const wrapper = await mountWizard()

    await wrapper.findAll('button').find((b) => b.text() === 'Edit')!.trigger('click')
    expect((wrapper.find('#hero-name').element as HTMLInputElement).value).toBe('Sherlock Holmes')

    await wrapper.find('#hero-name').setValue('Enola Holmes')
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(updateHero).toHaveBeenCalledWith(10, { name: 'Enola Holmes', imageUrl: null })
    expect(wrapper.text()).toContain('Enola Holmes')
    expect(wrapper.text()).not.toContain('Sherlock Holmes')
    expect(createHero).not.toHaveBeenCalled()
  })

  it('surfaces a failed save as an error', async () => {
    createHero.mockRejectedValue(new Error('Hero name already exists'))
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click')
    await wrapper.find('#hero-name').setValue('Dracula')
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Hero name already exists')
  })

  it('cancelling the form discards the draft and returns to the list', async () => {
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click')
    await wrapper.find('#hero-name').setValue('Draft Hero')
    await wrapper.find('button.btn-ghost').trigger('click')

    expect(wrapper.find('#hero-name').exists()).toBe(false)
    expect(createHero).not.toHaveBeenCalled()
  })
})
