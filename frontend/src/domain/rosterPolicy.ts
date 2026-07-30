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
