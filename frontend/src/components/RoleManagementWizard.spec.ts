import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { RosterRole } from '@/api/types'

const listRoles = vi.fn()
const createRole = vi.fn()
const updateRole = vi.fn()
const deleteRole = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    roles: (...args: unknown[]) => listRoles(...args),
    admin: {
      createRole: (...args: unknown[]) => createRole(...args),
      updateRole: (...args: unknown[]) => updateRole(...args),
      deleteRole: (...args: unknown[]) => deleteRole(...args),
    },
  },
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
  violationMessages: () => [],
}))

let rolesEnabled = true

vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({
    tournaments: [{ id: 1, name: 'Winter Open' }],
    byId: () => ({ id: 1, name: 'Winter Open', rolesEnabled }),
  }),
}))

const attacker: RosterRole = {
  id: 3,
  tournamentId: 1,
  name: 'Attacker',
  sortOrder: 1,
  weights: [{ stat: 'ATTACKS', coefficient: 1 }],
}
const healer: RosterRole = {
  id: 4,
  tournamentId: 1,
  name: 'Healer',
  maxPerRoster: 1,
  sortOrder: 2,
  weights: [{ stat: 'HEALING', coefficient: 1.5 }],
}

/** Mounts the wizard and picks the one seeded tournament, which is what triggers the listing. */
async function mountWizard() {
  const RoleManagementWizard = (await import('./RoleManagementWizard.vue')).default
  const wrapper = mount(RoleManagementWizard)
  await wrapper.find('#tournament-select').setValue('1')
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  rolesEnabled = true
  listRoles.mockResolvedValue([attacker, healer])
})

describe('RoleManagementWizard', () => {
  it("lists a tournament's roles with their caps and the stats they reward", async () => {
    const wrapper = await mountWizard()

    expect(listRoles).toHaveBeenCalledWith(1)
    expect(wrapper.text()).toContain('Attacker')
    expect(wrapper.text()).toContain('ATTACKS × 1')
    expect(wrapper.text()).toContain('Max 1')
    expect(wrapper.text()).not.toContain('Roles are switched off')
  })

  it('says so when the tournament has roles switched off', async () => {
    rolesEnabled = false
    const wrapper = await mountWizard()

    expect(wrapper.text()).toContain('Roles are switched off for Winter Open')
  })

  it('creates a role with its weights, an empty cap sent as uncapped', async () => {
    createRole.mockResolvedValue({ ...attacker, id: 9, name: 'Tactician' })
    const wrapper = await mountWizard()

    await wrapper.findAll('button').find((b) => b.text() === '+ Create Role')!.trigger('click')
    await wrapper.find('#role-name').setValue('Tactician')
    await wrapper.find('#role-stat-0').setValue('schemes_played')
    await wrapper.find('#role-weight-0').setValue('2')
    await wrapper.findAll('button').find((b) => b.text() === 'Save Role')!.trigger('click')
    await flushPromises()

    expect(createRole).toHaveBeenCalledWith(1, {
      name: 'Tactician',
      maxPerRoster: null,
      sortOrder: 3,
      weights: [{ stat: 'schemes_played', coefficient: 2 }],
    })
    expect(listRoles).toHaveBeenCalledTimes(2)
  })

  it('edits a role in place, keeping its position', async () => {
    updateRole.mockResolvedValue(healer)
    const wrapper = await mountWizard()

    await wrapper.find('#edit-role-4').trigger('click')
    expect((wrapper.find('#role-cap').element as HTMLInputElement).value).toBe('1')
    await wrapper.find('#role-name').setValue('Medic')
    await wrapper.findAll('button').find((b) => b.text() === 'Save Role')!.trigger('click')
    await flushPromises()

    expect(updateRole).toHaveBeenCalledWith(
      1,
      4,
      expect.objectContaining({ name: 'Medic', maxPerRoster: 1, sortOrder: 2 }),
    )
  })

  it('asks before deleting, and only deletes on confirmation', async () => {
    deleteRole.mockResolvedValue(undefined)
    const wrapper = await mountWizard()

    await wrapper.find('#delete-role-3').trigger('click')
    expect(deleteRole).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Delete “Attacker”?')

    await wrapper.findAll('button').find((b) => b.text() === 'Delete Role')!.trigger('click')
    await flushPromises()

    expect(deleteRole).toHaveBeenCalledWith(1, 3)
  })

  it('refuses a weight with no stat before asking the server', async () => {
    const wrapper = await mountWizard()

    await wrapper.findAll('button').find((b) => b.text() === '+ Create Role')!.trigger('click')
    await wrapper.find('#role-name').setValue('Blank')
    await wrapper.findAll('button').find((b) => b.text() === 'Save Role')!.trigger('click')
    await flushPromises()

    expect(createRole).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Every weight needs a stat name')
  })
})
