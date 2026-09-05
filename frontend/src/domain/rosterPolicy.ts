import type { BudgetStatus } from '@/api/types'

/**
 * Client-side mirror of the backend's `RosterPolicy` budget arithmetic.
 *
 * This exists purely so the budget meter responds the instant a hero is
 * clicked, without waiting for a round trip. The server remains authoritative —
 * it recomputes the same values and rejects anything invalid at lock time — so
 * if the two ever disagree, the server wins.
 */
export function budgetStatus(costs: number[], creditGrant: number): BudgetStatus {
  const spent = costs.reduce((total, cost) => total + cost, 0)
  return {
    spent,
    creditGrant,
    remaining: creditGrant - spent,
    utilisation: creditGrant > 0 ? spent / creditGrant : 0,
  }
}

/**
 * Client-side mirror of the backend's `roster_policy::swap_allowance`.
 *
 * A window follows a round, so at round 1 there is nothing to react to and the
 * allowance is zero. Nothing is banked explicitly: an unused window simply
 * never spends, which is what makes carry-over fall out of the subtraction.
 *
 * The server still decides — `Roster.swapsAvailable` is the authoritative
 * figure, and this exists so the panel can count *down* as heroes are staged,
 * before anything is submitted.
 */
export function swapAllowance(
  swapsPerRound: number,
  currentRound: number,
  swapsUsedTotal: number,
): number {
  const windowsOffered = Math.max(currentRound - 1, 0)
  return Math.max(swapsPerRound * windowsOffered - swapsUsedTotal, 0)
}

/**
 * How many heroes a proposed roster would bring in — the number of swaps a
 * submission actually spends.
 *
 * Counted on the *arriving* side, not as a symmetric difference: an exchange
 * moves one hero out and one in, so counting both would double every swap.
 * A roster that changes only its order spends nothing.
 */
export function heroesChanged(currentHeroIds: number[], proposedHeroIds: number[]): number {
  const held = new Set(currentHeroIds)
  return proposedHeroIds.filter((id) => !held.has(id)).length
}
