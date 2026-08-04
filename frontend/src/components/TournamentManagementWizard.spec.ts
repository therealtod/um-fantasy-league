import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Tournament } from '@/api/types'

const createTournament = vi.fn()
const updateTournament = vi.fn()
const deleteTournament = vi.fn()
const storeLoad = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      createTournament: (...args: unknown[]) => createTournament(...args),
      updateTournament: (...args: unknown[]) => updateTournament(...args),
      deleteTournament: (...args: unknown[]) => deleteTournament(...args),
    },
  },
}))

const tournaments: Tournament[] = [
  {
    id: 1,
    name: 'Winter Open',
    format: 'BANQUEST',
    status: 'REGISTRATION_OPEN',
    startDate: '2026-09-01',
    endDate: null,
    capacity: 64,
    enrolled: 12,
    rosterSize: 3,
    creditGrant: 10000,
    acceptsRegistration: true,
  },
]

vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({ tournaments, load: storeLoad }),
}))

async function mountWizard() {
  const TournamentManagementWizard = (await import('./TournamentManagementWizard.vue')).default
  const wrapper = mount(TournamentManagementWizard)
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.spyOn(window, 'confirm').mockReturnValue(true)
  storeLoad.mockResolvedValue(undefined)
})

describe('TournamentManagementWizard', () => {
  it('lists tournaments from the store', async () => {
    const wrapper = await mountWizard()

    expect(wrapper.text()).toContain('Winter Open')
    expect(wrapper.text()).toContain('12/64')
  })

  it('creates a tournament from the form and refreshes the store', async () => {
    createTournament.mockResolvedValue({})
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click') // + Create Tournament
    await wrapper.find('#tournament-name').setValue('Summer of Legends')
    await wrapper.find('#tournament-start').setValue('2026-10-01')
    await wrapper.find('button.btn-primary').trigger('click') // Save Tournament
    await flushPromises()

    expect(createTournament).toHaveBeenCalledWith(
      expect.objectContaining({ name: 'Summer of Legends', startDate: '2026-10-01' }),
    )
    expect(storeLoad).toHaveBeenCalled()
    // The form closes back to the list once saved.
    expect(wrapper.find('#tournament-name').exists()).toBe(false)
  })

  it('refuses to save without a name or a start date', async () => {
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click')
    await wrapper.find('button.btn-primary').trigger('click')

    expect(createTournament).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Tournament name is required')

    await wrapper.find('#tournament-name').setValue('Summer of Legends')
    await wrapper.find('button.btn-primary').trigger('click')

    expect(createTournament).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Start date is required')
  })

  it('prefills the form from the existing tournament on edit', async () => {
    updateTournament.mockResolvedValue({})
    const wrapper = await mountWizard()

    await wrapper.findAll('button').find((b) => b.text() === 'Edit')!.trigger('click')

    expect((wrapper.find('#tournament-name').element as HTMLInputElement).value).toBe('Winter Open')
    expect((wrapper.find('#tournament-start').element as HTMLInputElement).value).toBe('2026-09-01')

    await wrapper.find('#tournament-capacity').setValue('96')
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(updateTournament).toHaveBeenCalledWith(1, expect.objectContaining({ capacity: 96 }))
    expect(storeLoad).toHaveBeenCalled()
  })

  it('asks for confirmation before deleting, then deletes and refreshes the store', async () => {
    deleteTournament.mockResolvedValue(undefined)
    const wrapper = await mountWizard()

    await wrapper.findAll('button').find((b) => b.text() === 'Delete')!.trigger('click')

    expect(deleteTournament).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Delete Tournament')
    expect(wrapper.text()).toContain('Winter Open')

    await wrapper.findAll('button').find((b) => b.text() === 'Delete Tournament')!.trigger('click')
    await flushPromises()

    expect(deleteTournament).toHaveBeenCalledWith(1)
    expect(storeLoad).toHaveBeenCalled()
  })

  it('cancelling delete leaves the tournament untouched', async () => {
    const wrapper = await mountWizard()

    await wrapper.findAll('button').find((b) => b.text() === 'Delete')!.trigger('click')
    await wrapper.find('button.btn-ghost').trigger('click')

    expect(deleteTournament).not.toHaveBeenCalled()
    expect(wrapper.text()).not.toContain('Delete Tournament')
  })

  it('surfaces a failed save as an error', async () => {
    createTournament.mockRejectedValue(new Error('Tournament name already exists'))
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click')
    await wrapper.find('#tournament-name').setValue('Winter Open')
    await wrapper.find('#tournament-start').setValue('2026-10-01')
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Tournament name already exists')
  })
})
