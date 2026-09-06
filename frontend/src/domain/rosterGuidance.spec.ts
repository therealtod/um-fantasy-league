import { describe, expect, it } from 'vitest'
import {
  finalStepLabel,
  lockBlockedReason,
  nextStep,
  rosterStage,
  swapBlockedReason,
  type RosterState,
} from './rosterGuidance'

/** A registered, empty, in-budget entry — each test varies one thing off this. */
function state(overrides: Partial<RosterState> = {}): RosterState {
  return {
    registered: true,
    locked: false,
    picked: 0,
    rosterSize: 3,
    remaining: 10_000,
    creditGrant: 10_000,
    swapWindowOpen: false,
    swapsAvailable: 0,
    alreadySwappedThisRound: false,
    swapsStaged: 0,
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
    const step = nextStep(state({ registered: false }), true)

    expect(step.title).toBe('Register to start drafting')
    expect(step.detail).toContain('10,000 CR')
    expect(step.detail).toContain('3 heroes')
  })

  it('counts down the picks and the budget left', () => {
    const step = nextStep(state({ picked: 2, remaining: 1_500 }), true)

    expect(step.title).toBe('Pick 1 more hero')
    expect(step.detail).toContain('1,500 CR')
  })

  it('leads with the overspend when a full roster is too expensive', () => {
    const step = nextStep(state({ picked: 3, remaining: -2_800 }), true)

    expect(step.title).toBe('You are 2,800 CR over budget')
    expect(step.detail).toContain('cheaper')
  })

  it('warns that an unlocked entry is dropped when prompting the lock', () => {
    const step = nextStep(state({ picked: 3, remaining: 600 }), true)

    expect(step.title).toBe('Lock in your roster')
    expect(step.detail).toContain('removed when the tournament goes live')
  })

  it('points a locked entry at the standings once the board is open', () => {
    const step = nextStep(state({ locked: true, picked: 3 }), true)

    expect(step.title).toContain('locked')
    expect(step.detail).toContain('Follow them on the standings page')
  })

  // The standings page only lists LIVE/COMPLETED tournaments, so a manager who
  // locks during registration would otherwise be sent to an empty state.
  it('says when the board opens instead, while the tournament is pre-live', () => {
    const step = nextStep(state({ locked: true, picked: 3 }), false)

    expect(step.title).toContain('locked')
    expect(step.detail).toContain('opens then')
    expect(step.detail).not.toContain('Follow them on the standings page')
  })
})

describe('finalStepLabel', () => {
  it('names the board only when there is one to watch', () => {
    expect(finalStepLabel(true)).toBe('Watch standings')
    expect(finalStepLabel(false)).toBe('Await go-live')
  })
})

/** A locked entry inside an open window with one swap to spend. */
function inWindow(overrides: Partial<RosterState> = {}): RosterState {
  return state({
    locked: true,
    picked: 3,
    swapWindowOpen: true,
    swapsAvailable: 1,
    ...overrides,
  })
}

describe('the swap stage', () => {
  it('leaves a locked roster at DONE while no window is open', () => {
    expect(rosterStage(state({ locked: true, picked: 3 }))).toBe('DONE')
  })

  it('moves to SWAP when a window opens on a locked roster', () => {
    expect(rosterStage(inWindow())).toBe('SWAP')
  })

  it('falls back to DONE once the round\u2019s submission is spent', () => {
    expect(rosterStage(inWindow({ alreadySwappedThisRound: true }))).toBe('DONE')
  })

  it('falls back to DONE when the allowance is exhausted', () => {
    expect(rosterStage(inWindow({ swapsAvailable: 0 }))).toBe('DONE')
  })

  it('never reaches SWAP from an unlocked entry, however open the window', () => {
    expect(rosterStage(inWindow({ locked: false, remaining: 4_000 }))).toBe('LOCK')
  })

  it('stops claiming there is nothing more to do', () => {
    // The regression this stage exists for: DONE's copy is false the moment a
    // window opens, and a manager reading it would sit out their swap.
    // A window only opens between rounds of a LIVE tournament, so the board is
    // always there: `standingsOpen` is true for every case in this block.
    expect(nextStep(inWindow(), true).title).not.toContain('nothing more to do')
    expect(nextStep(inWindow(), true).title).toContain('window is open')
  })

  it('asks for a submission once something is staged', () => {
    expect(nextStep(inWindow({ swapsStaged: 1 }), true).title).toBe('Submit 1 swap')
    expect(nextStep(inWindow({ swapsStaged: 2, swapsAvailable: 2 }), true).title).toBe(
      'Submit 2 swaps',
    )
  })

  it('says a submission is final before it is made, not after', () => {
    expect(nextStep(inWindow({ swapsStaged: 1 }), true).detail).toContain('one submission')
  })
})

describe('swapBlockedReason', () => {
  it('is null when a staged, in-budget exchange is ready to send', () => {
    expect(swapBlockedReason(inWindow({ swapsStaged: 1 }))).toBeNull()
  })

  it('sends an unlocked entry back to the draft path', () => {
    expect(swapBlockedReason(state({ locked: false }))).toContain('Lock your roster')
  })

  it('names the shut window ahead of anything else that is also wrong', () => {
    const shut = inWindow({ swapWindowOpen: false, swapsStaged: 0, remaining: -500 })
    expect(swapBlockedReason(shut)).toContain('window is closed')
  })

  it('reports the spent submission rather than the remaining allowance', () => {
    const spent = inWindow({ alreadySwappedThisRound: true, swapsAvailable: 2 })
    expect(swapBlockedReason(spent)).toContain('already submitted')
  })

  it('reports an exhausted allowance', () => {
    expect(swapBlockedReason(inWindow({ swapsAvailable: 0 }))).toContain('no swaps left')
  })

  it('reports an over-budget exchange in credits', () => {
    expect(swapBlockedReason(inWindow({ remaining: -1_500 }))).toContain('over budget')
  })

  it('insists the roster stays the size it was', () => {
    expect(swapBlockedReason(inWindow({ picked: 2 }))).toContain('exchange')
  })

  it('asks for a staged change when nothing has been touched', () => {
    expect(swapBlockedReason(inWindow({ swapsStaged: 0 }))).toContain('Choose a hero')
  })

  it('refuses more staged swaps than the allowance covers', () => {
    const over = inWindow({ swapsAvailable: 1, swapsStaged: 2 })
    expect(swapBlockedReason(over)).toBe('You have staged 2 swaps but only 1 swap available.')
  })
})
