import { describe, expect, it } from 'vitest'
import { budgetStatus, heroesChanged, swapAllowance } from './rosterPolicy'

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

describe('swapAllowance', () => {
  it('offers nothing in round one — a window follows a round', () => {
    expect(swapAllowance(2, 1, 0)).toBe(0)
  })

  it('offers one round of allowance per round already played', () => {
    expect(swapAllowance(2, 2, 0)).toBe(2)
    expect(swapAllowance(2, 4, 0)).toBe(6)
  })

  it('banks an unused window rather than expiring it', () => {
    // Three windows offered at one swap each, none spent: all three are still
    // there. Carry-over is not a feature of the arithmetic so much as what the
    // arithmetic does when nothing subtracts from it.
    expect(swapAllowance(1, 4, 0)).toBe(3)
  })

  it('subtracts every swap ever made, not just this round’s', () => {
    expect(swapAllowance(1, 4, 2)).toBe(1)
  })

  it('never goes negative, however the configuration is retuned', () => {
    // An admin lowering `swapsPerRound` after swaps were made can put the
    // subtraction underwater; that is zero swaps left, not a debt.
    expect(swapAllowance(1, 2, 5)).toBe(0)
  })

  it('switches the mechanic off entirely at zero per round', () => {
    expect(swapAllowance(0, 9, 0)).toBe(0)
  })
})

describe('heroesChanged', () => {
  it('counts one swap for a one-for-one exchange, not two', () => {
    expect(heroesChanged([1, 2, 3], [1, 2, 9])).toBe(1)
  })

  it('counts the arriving heroes when several move at once', () => {
    expect(heroesChanged([1, 2, 3], [1, 8, 9])).toBe(2)
  })

  it('charges nothing for reordering the same heroes', () => {
    expect(heroesChanged([1, 2, 3], [3, 1, 2])).toBe(0)
  })

  it('charges nothing for an unchanged roster', () => {
    expect(heroesChanged([1, 2, 3], [1, 2, 3])).toBe(0)
  })
})
