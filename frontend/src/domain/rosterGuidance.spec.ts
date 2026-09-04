import { describe, expect, it } from 'vitest'
import { lockBlockedReason, nextStep, rosterStage, type RosterState } from './rosterGuidance'

/** A registered, empty, in-budget entry — each test varies one thing off this. */
function state(overrides: Partial<RosterState> = {}): RosterState {
  return {
    registered: true,
    locked: false,
    picked: 0,
    rosterSize: 3,
    remaining: 10_000,
    creditGrant: 10_000,
    ...overrides,
  }
}

describe('rosterStage', () => {
  it('asks for registration before anything else', () => {
    expect(rosterStage(state({ registered: false }))).toBe('REGISTER')
  })

  it('stays on picking while slots are empty', () => {
    expect(rosterStage(state({ picked: 2, remaining: 4_000 }))).toBe('PICK')
  })

  it('stays on picking when a full roster is over budget', () => {
    expect(rosterStage(state({ picked: 3, remaining: -800 }))).toBe('PICK')
  })

  it('advances to locking once the roster is full and affordable', () => {
    expect(rosterStage(state({ picked: 3, remaining: 600 }))).toBe('LOCK')
  })

  it('reports a locked entry as done', () => {
    expect(rosterStage(state({ locked: true, picked: 3, remaining: 600 }))).toBe('DONE')
  })

  it('reports a locked entry as done even if it would otherwise look unfinished', () => {
    expect(rosterStage(state({ locked: true, picked: 0 }))).toBe('DONE')
  })
})

describe('lockBlockedReason', () => {
  it('names registration as the blocker before an entry exists', () => {
    expect(lockBlockedReason(state({ registered: false }))).toBe(
      'Register first to start picking heroes.',
    )
  })

  it('counts the remaining picks, singular', () => {
    expect(lockBlockedReason(state({ picked: 2 }))).toBe('Pick 1 more hero to complete your roster.')
  })

  it('counts the remaining picks, plural', () => {
    expect(lockBlockedReason(state({ picked: 1 }))).toBe(
      'Pick 2 more heroes to complete your roster.',
    )
  })

  it('reports the overspend once the roster is full', () => {
    expect(lockBlockedReason(state({ picked: 3, remaining: -2_800 }))).toBe(
      'You are 2,800 CR over budget. Swap a hero for a cheaper one.',
    )
  })

  it('prefers the incomplete-roster reason over the budget one', () => {
    expect(lockBlockedReason(state({ picked: 2, remaining: -500 }))).toBe(
      'Pick 1 more hero to complete your roster.',
    )
  })

  it('returns null for a lockable roster', () => {
    expect(lockBlockedReason(state({ picked: 3, remaining: 0 }))).toBeNull()
  })

  it('returns null for an already locked roster', () => {
    expect(lockBlockedReason(state({ locked: true, picked: 3 }))).toBeNull()
  })
})

describe('nextStep', () => {
  it('explains what registering buys', () => {
    const step = nextStep(state({ registered: false }))

    expect(step.title).toBe('Register to start drafting')
    expect(step.detail).toContain('10,000 CR')
    expect(step.detail).toContain('3 heroes')
  })

  it('counts down the picks and the budget left', () => {
    const step = nextStep(state({ picked: 2, remaining: 1_500 }))

    expect(step.title).toBe('Pick 1 more hero')
    expect(step.detail).toContain('1,500 CR')
  })

  it('leads with the overspend when a full roster is too expensive', () => {
    const step = nextStep(state({ picked: 3, remaining: -2_800 }))

    expect(step.title).toBe('You are 2,800 CR over budget')
    expect(step.detail).toContain('cheaper')
  })

  it('warns that an unlocked entry is dropped when prompting the lock', () => {
    const step = nextStep(state({ picked: 3, remaining: 600 }))

    expect(step.title).toBe('Lock in your roster')
    expect(step.detail).toContain('removed when the tournament goes live')
  })

  it('points a locked entry at the standings', () => {
    const step = nextStep(state({ locked: true, picked: 3 }))

    expect(step.title).toContain('locked')
    expect(step.detail).toContain('standings')
  })
})
