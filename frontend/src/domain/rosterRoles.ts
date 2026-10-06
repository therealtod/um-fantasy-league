/**
 * The roster-role rules the builder can state without a DOM: whether a roster's
 * roles would let it lock, and which assignments a submission should carry.
 *
 * Mirrors `umfl_domain::roster_roles::validate_assignments` the way
 * `rosterPolicy.ts` mirrors the budget arithmetic — so the lock button and its
 * hint react on click — while the server stays authoritative and re-checks
 * every rule. **If you change a rule here, change it there too.**
 */
import type { RoleAssignment, RosterRole } from '@/api/types'

/** heroId → roleId, for the heroes that have one. */
export type RoleMap = Record<number, number>

export function toRoleMap(assignments: RoleAssignment[]): RoleMap {
  const map: RoleMap = {}
  for (const { heroId, roleId } of assignments) map[heroId] = roleId
  return map
}

/** The roles of `heroIds` that have one, in slot order — the body `setRoles` wants. */
export function assignmentsFor(heroIds: number[], roles: RoleMap): RoleAssignment[] {
  return heroIds
    .filter((heroId) => roles[heroId] !== undefined)
    .map((heroId) => ({ heroId, roleId: roles[heroId]! }))
}

/**
 * What a swap submission should carry: a role for every arriving hero, and for
 * any kept hero whose role the manager changed. A kept hero whose role is
 * unchanged is left out — the server keeps what it already has.
 */
export function swapRoles(
  heldIds: number[],
  proposedIds: number[],
  held: RoleMap,
  staged: RoleMap,
): RoleAssignment[] {
  return assignmentsFor(proposedIds, staged).filter(
    ({ heroId, roleId }) => !heldIds.includes(heroId) || held[heroId] !== roleId,
  )
}

/**
 * Why these roles would stop a commit, one line per problem, in the server's
 * order: heroes still without a role, then any role over its cap.
 *
 * `requireComplete` names the heroes that must have a role — the whole roster at
 * lock, the arrivals on a swap. Empty when roles are off.
 */
export function roleProblems(
  heroIds: number[],
  roles: RoleMap,
  available: RosterRole[],
  requireComplete: number[],
): string[] {
  const problems: string[] = []

  const unassigned = requireComplete.filter((heroId) => roles[heroId] === undefined).length
  if (unassigned > 0) {
    problems.push(
      unassigned === 1 ? 'One hero still needs a role.' : `${unassigned} heroes still need a role.`,
    )
  }

  for (const role of available) {
    if (role.maxPerRoster === undefined) continue
    const taken = heroIds.filter((heroId) => roles[heroId] === role.id).length
    if (taken > role.maxPerRoster) {
      problems.push(`At most ${role.maxPerRoster} ${role.name} — ${taken} assigned.`)
    }
  }

  return problems
}

/** "ATTACKS ×1 · DAMAGE DEALT ×0.25" — what a role rewards, as a one-line hint. */
export function describeWeights(role: RosterRole): string {
  if (role.weights.length === 0) return 'Rewards nothing yet'
  return role.weights
    .map(({ stat, coefficient }) => `${stat.replaceAll('_', ' ')} ×${coefficient}`)
    .join(' · ')
}
