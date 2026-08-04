import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { ScoringRuleSetDto } from '@/api/types'

const listScoringRuleSets = vi.fn()
const createScoringRuleSet = vi.fn()
const updateScoringRuleSet = vi.fn()
const activateScoringRuleSet = vi.fn()

vi.mock('@/api/client', () => ({
  api: {
    admin: {
      listScoringRuleSets: (...args: unknown[]) => listScoringRuleSets(...args),
      createScoringRuleSet: (...args: unknown[]) => createScoringRuleSet(...args),
      updateScoringRuleSet: (...args: unknown[]) => updateScoringRuleSet(...args),
      activateScoringRuleSet: (...args: unknown[]) => activateScoringRuleSet(...args),
    },
  },
}))

vi.mock('@/stores/tournaments', () => ({
  useTournamentsStore: () => ({ tournaments: [{ id: 1, name: 'Winter Open' }] }),
}))

const standard: ScoringRuleSetDto = {
  id: 7,
  tournamentId: 1,
  name: 'Standard Scoring',
  isActive: true,
  coefficients: [
    { metric: 'WIN', coefficient: 3, sortOrder: 0 },
    { metric: 'APPEARANCE', coefficient: 1, sortOrder: 1 },
  ],
  warnings: [],
}

const experimental: ScoringRuleSetDto = {
  id: 8,
  tournamentId: 1,
  name: 'Experimental',
  isActive: false,
  coefficients: [{ metric: 'CROWD_FAVOURITE', coefficient: 2, sortOrder: 0 }],
  warnings: ['CROWD_FAVOURITE'],
}

/** Mounts the wizard and picks the one seeded tournament, which is what triggers the listing. */
async function mountWizard() {
  const ScoringRuleSetWizard = (await import('./ScoringRuleSetWizard.vue')).default
  const wrapper = mount(ScoringRuleSetWizard)
  await wrapper.find('#tournament-select').setValue('1')
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  listScoringRuleSets.mockResolvedValue([standard, experimental])
})

describe('ScoringRuleSetWizard', () => {
  it('lists a tournament\'s rule sets with their active flag', async () => {
    const wrapper = await mountWizard()

    expect(listScoringRuleSets).toHaveBeenCalledWith(1)
    expect(wrapper.text()).toContain('Standard Scoring')
    expect(wrapper.text()).toContain('Experimental')
    // Only the inactive one offers activation; the active one has nothing to switch to.
    expect(wrapper.find('#activate-8').exists()).toBe(true)
    expect(wrapper.find('#activate-7').exists()).toBe(false)
  })

  it('activates a rule set and reloads, since activating deactivates its sibling', async () => {
    activateScoringRuleSet.mockResolvedValue({ ...experimental, isActive: true })
    const wrapper = await mountWizard()
    listScoringRuleSets.mockResolvedValue([
      { ...experimental, isActive: true },
      { ...standard, isActive: false },
    ])

    await wrapper.find('#activate-8').trigger('click')
    await flushPromises()

    expect(activateScoringRuleSet).toHaveBeenCalledWith(1, 8)
    expect(listScoringRuleSets).toHaveBeenCalledTimes(2)
    expect(wrapper.find('#activate-7').exists()).toBe(true)
    expect(wrapper.find('#activate-8').exists()).toBe(false)
  })

  it('edits an existing rule set through updateScoringRuleSet', async () => {
    updateScoringRuleSet.mockResolvedValue({ ...standard, name: 'Standard v2' })
    const wrapper = await mountWizard()

    await wrapper.find('#edit-7').trigger('click')
    // The form is seeded from the rule set rather than starting blank.
    expect((wrapper.find('#rule-set-name').element as HTMLInputElement).value).toBe('Standard Scoring')
    expect((wrapper.find('#metric-0').element as HTMLInputElement).value).toBe('WIN')

    await wrapper.find('#rule-set-name').setValue('Standard v2')
    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(createScoringRuleSet).not.toHaveBeenCalled()
    expect(updateScoringRuleSet).toHaveBeenCalledWith(1, 7, {
      name: 'Standard v2',
      coefficients: [
        { metric: 'WIN', coefficient: 3, sortOrder: 0 },
        { metric: 'APPEARANCE', coefficient: 1, sortOrder: 1 },
      ],
    })
  })

  it('renders the warnings a save comes back with instead of swallowing them', async () => {
    createScoringRuleSet.mockResolvedValue({
      ...experimental,
      name: 'Typo Set',
      warnings: ['HEALH_REMAINING'],
    })
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click') // + Create Scoring Rule Set
    await wrapper.find('#rule-set-name').setValue('Typo Set')
    await wrapper.find('#metric-0').setValue('HEALH_REMAINING')
    await wrapper.find('button.btn-primary').trigger('click') // Save Rule Set
    await flushPromises()

    expect(wrapper.text()).toContain('HEALH_REMAINING')
    expect(wrapper.text()).toContain('will score zero')
  })

  it('flags an unknown metric in the form without blocking the save', async () => {
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click')
    await wrapper.find('#metric-0').setValue('HEALH_REMAINING')

    expect(wrapper.text()).toContain('no extractor implements')
    // A known metric — normalised the same way MatchMetrics does — draws no warning.
    await wrapper.find('#metric-0').setValue(' health_remaining ')
    expect(wrapper.text()).not.toContain('no extractor implements')
  })

  it('renumbers sortOrder when a coefficient is added or removed', async () => {
    createScoringRuleSet.mockResolvedValue({ ...standard, warnings: [] })
    const wrapper = await mountWizard()

    await wrapper.find('button.btn-primary').trigger('click')
    await wrapper.find('#rule-set-name').setValue('Three Rows')

    // Start with one row, add two more: sortOrder tracks position.
    await wrapper.findAll('button.btn-ghost').find((b) => b.text() === '+ Add Coefficient')!.trigger('click')
    await wrapper.findAll('button.btn-ghost').find((b) => b.text() === '+ Add Coefficient')!.trigger('click')
    await wrapper.find('#metric-0').setValue('WIN')
    await wrapper.find('#metric-1').setValue('LOSS')
    await wrapper.find('#metric-2').setValue('DRAW')
    expect((wrapper.find('#sort-2').element as HTMLInputElement).value).toBe('2')

    // Removing the middle row closes the gap rather than leaving 0, 2.
    await wrapper.findAll('button.btn-ghost').filter((b) => b.text() === 'Remove')[1]!.trigger('click')

    expect((wrapper.find('#metric-1').element as HTMLInputElement).value).toBe('DRAW')
    expect((wrapper.find('#sort-1').element as HTMLInputElement).value).toBe('1')
    expect(wrapper.find('#metric-2').exists()).toBe(false)

    await wrapper.find('button.btn-primary').trigger('click')
    await flushPromises()

    expect(createScoringRuleSet).toHaveBeenCalledWith(1, {
      name: 'Three Rows',
      coefficients: [
        { metric: 'WIN', coefficient: 1, sortOrder: 0 },
        { metric: 'DRAW', coefficient: 1, sortOrder: 1 },
      ],
      activate: false,
    })
  })
})
