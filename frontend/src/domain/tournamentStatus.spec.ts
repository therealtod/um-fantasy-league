import { describe, expect, it } from 'vitest'
import type { Tournament } from '@/api/types'
import { standingsAvailable, swapInviteOpen } from './tournamentStatus'

describe('standingsAvailable', () => {
  it('offers a board once the tournament is live or finished', () => {
    expect(standingsAvailable('LIVE')).toBe(true)
    expect(standingsAvailable('COMPLETED')).toBe(true)
  })

  it('offers none before the first result can exist', () => {
    expect(standingsAvailable('SCHEDULED')).toBe(false)
    expect(standingsAvailable('REGISTRATION_OPEN')).toBe(false)
  })

  it('treats an unknown tournament as having no board', () => {
    expect(standingsAvailable(null)).toBe(false)
    expect(standingsAvailable(undefined)).toBe(false)
  })
})

/**
 * A live tournament, past round 1, with a window open and a locked entry —
 * every field the predicate reads set to the state that should invite a swap.
 * Each case below negates exactly one of them.
 */
function invited(overrides: Partial<Tournament> = {}): Tournament {
  return {
    id: 7,
    name: 'Winter of Champions',
    format: 'BANQUEST',
    status: 'LIVE',
    startDate: '2026-01-10',
    capacity: 16,
    enrolled: 12,
    rosterSize: 3,
    creditGrant: 10_000,
    acceptsRegistration: false,
    currentRound: 3,
    swapsPerRound: 1,
    swapWindowOpen: true,
    myEntryStatus: 'LOCKED',
    ...overrides,
  }
}

describe('swapInviteOpen', () => {
  it('invites a locked entry when a window is open past round 1', () => {
    expect(swapInviteOpen(invited())).toBe(true)
  })

  it('declines when the admin has shut the window', () => {
    expect(swapInviteOpen(invited({ swapWindowOpen: false }))).toBe(false)
  })

  it('declines anyone without a locked entry', () => {
    expect(swapInviteOpen(invited({ myEntryStatus: 'DRAFT' }))).toBe(false)
    expect(swapInviteOpen(invited({ myEntryStatus: undefined }))).toBe(false)
    expect(swapInviteOpen(invited({ myEntryStatus: null }))).toBe(false)
  })

  it('declines when the mechanic is switched off', () => {
    expect(swapInviteOpen(invited({ swapsPerRound: 0 }))).toBe(false)
  })

  it('declines in round 1, where no window has elapsed yet', () => {
    expect(swapInviteOpen(invited({ currentRound: 1 }))).toBe(false)
  })

  it('declines on a finished tournament even with the flag left open', () => {
    expect(swapInviteOpen(invited({ status: 'COMPLETED' }))).toBe(false)
  })
})
