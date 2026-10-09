import { describe, expect, it } from 'vitest'
import type { RosterRole } from '@/api/types'
import {
  assignmentsFor,
  describeWeights,
  overCapHeroIds,
  roleProblems,
  roleUsage,
  swapRoles,
  toRoleMap,
} from './rosterRoles'

const ATTACKER = 1
const HEALER = 2

const roles: RosterRole[] = [
  {
    id: ATTACKER,
    tournamentId: 7,
    name: 'Attacker',
    sortOrder: 1,
    weights: [
      { stat: 'ATTACKS', coefficient: 1 },
      { stat: 'DAMAGE_DEALT', coefficient: 0.25 },
    ],
  },
  { id: HEALER, tournamentId: 7, name: 'Healer', maxPerRoster: 1, sortOrder: 2, weights: [] },
]

describe('roleProblems', () => {
  it('passes a fully assigned roster within every cap', () => {
    expect(roleProblems([10, 11], { 10: ATTACKER, 11: HEALER }, roles, [10, 11])).toEqual([])
  })

  it('counts the heroes still without a role, but only the ones that must have one', () => {
    expect(roleProblems([10, 11, 12], { 10: ATTACKER }, roles, [10, 11, 12])).toEqual([
      '2 heroes still need a role.',
    ])
    expect(roleProblems([10, 11], { 10: ATTACKER }, roles, [11])).toEqual([
      'One hero still needs a role.',
    ])
    expect(roleProblems([10, 11], {}, roles, []), 'a draft is a scratchpad').toEqual([])
  })

  it('names a role over its cap, and ignores an uncapped one', () => {
    expect(roleProblems([10, 11], { 10: HEALER, 11: HEALER }, roles, [])).toEqual([
      'At most 1 Healer — 2 assigned.',
    ])
    expect(roleProblems([10, 11, 12], { 10: ATTACKER, 11: ATTACKER, 12: ATTACKER }, roles, [])).toEqual([])
  })

  it('does not count a stale role for a hero no longer on the roster', () => {
    expect(roleProblems([10], { 10: HEALER, 99: HEALER }, roles, [10])).toEqual([])
  })
})

describe('roleUsage and overCapHeroIds', () => {
  it('counts each role among the roster, ignoring heroes without one and stale entries', () => {
    const usage = roleUsage([10, 11, 12], { 10: ATTACKER, 11: ATTACKER, 99: HEALER })
    expect([...usage]).toEqual([[ATTACKER, 2]])
  })

  it('flags every holder of an over-cap role, and nobody under a cap or uncapped', () => {
    const map = { 10: HEALER, 11: HEALER, 12: ATTACKER, 13: ATTACKER }
    expect([...overCapHeroIds([10, 11, 12, 13], map, roles)]).toEqual([10, 11])
    expect(overCapHeroIds([10, 12], map, roles).size).toBe(0)
  })
})

describe('assignments', () => {
  it('round-trips the wire shape, in slot order, skipping heroes without a role', () => {
    const map = toRoleMap([
      { heroId: 11, roleId: HEALER },
      { heroId: 10, roleId: ATTACKER },
    ])

    expect(assignmentsFor([10, 12, 11], map)).toEqual([
      { heroId: 10, roleId: ATTACKER },
      { heroId: 11, roleId: HEALER },
    ])
  })

  it('sends a swap the arrivals and the kept heroes whose role changed, nothing else', () => {
    const held = { 10: ATTACKER, 11: HEALER, 12: ATTACKER }
    const staged = { 10: ATTACKER, 11: ATTACKER, 13: HEALER }

    expect(swapRoles([10, 11, 12], [10, 11, 13], held, staged)).toEqual([
      { heroId: 11, roleId: ATTACKER },
      { heroId: 13, roleId: HEALER },
    ])
  })
})

describe('describeWeights', () => {
  it('reads a role as the stats it rewards', () => {
    expect(describeWeights(roles[0]!)).toBe('ATTACKS ×1 · DAMAGE DEALT ×0.25')
    expect(describeWeights(roles[1]!)).toBe('Rewards nothing yet')
  })
})
