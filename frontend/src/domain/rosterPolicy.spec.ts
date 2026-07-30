import { describe, expect, it } from 'vitest'
import { budgetStatus } from './rosterPolicy'

/**
 * These mirror `RosterPolicyTest` on the backend. If one side changes its
 * budget arithmetic, the other should fail here.
 */
describe('budgetStatus', () => {
  it('reports an empty roster as fully unspent', () => {
    expect(budgetStatus([], 10_000)).toEqual({
      spent: 0,
      creditGrant: 10_000,
      remaining: 10_000,
      utilisation: 0,
    })
  })

  it('tracks spend and headroom', () => {
    const status = budgetStatus([4_100, 3_200, 2_100], 10_000)

    expect(status.spent).toBe(9_400)
    expect(status.remaining).toBe(600)
    expect(status.utilisation).toBeCloseTo(0.94)
  })

  it('goes negative when a premium trio is picked', () => {
    const status = budgetStatus([4_500, 3_200, 5_100], 10_000)

    expect(status.spent).toBe(12_800)
    expect(status.remaining).toBe(-2_800)
    expect(status.utilisation).toBeCloseTo(1.28)
  })

  it('reports zero utilisation rather than dividing by a zero grant', () => {
    expect(budgetStatus([1_000], 0)).toEqual({
      spent: 1_000,
      creditGrant: 0,
      remaining: -1_000,
      utilisation: 0,
    })
  })
})
