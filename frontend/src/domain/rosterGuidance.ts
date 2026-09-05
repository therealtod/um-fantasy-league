import { formatCredits } from '@/lib/format'

/**
 * Where a manager stands in the four-step entry flow, and what the page should
 * tell them to do next.
 *
 * This is the roster page's guidance half, kept out of the components for the
 * same reason `matchForm.ts` is kept out of `MatchResultWizard.vue`: it is
 * plain data in, plain strings out, so the copy for every state can be pinned
 * by a test rather than read off a rendered DOM.
 *
 * The stage predicates mirror `useRosterStore`'s `registered`/`locked`/`full`/
 * `lockable` computeds — `lockBlockedReason` in particular is the explanation
 * for exactly the condition that disables the lock button, so if that store
 * changes its mind about what makes a roster lockable, this must follow.
 */

/**
 * The steps of an entry, in order.
 *
 * `SWAP` is not a fifth step so much as a state a locked entry re-enters every
 * time an admin opens a window: it sits *after* `DONE` in the flow but is not
 * terminal, and a roster passes through it once per round for the rest of the
 * tournament.
 */
export type RosterStage = 'REGISTER' | 'PICK' | 'LOCK' | 'DONE' | 'SWAP'

export interface RosterState {
  registered: boolean
  locked: boolean
  picked: number
  rosterSize: number
  /** `BudgetStatus.remaining` — negative when the selection is over budget. */
  remaining: number
  creditGrant: number
  /** `Roster.swapWindowOpen` — whether an admin has a window open right now. */
  swapWindowOpen: boolean
  /** `Roster.swapsAvailable`, carry-over included. */
  swapsAvailable: number
  /** `Roster.alreadySwappedThisRound` — one submission per window, not per hero. */
  alreadySwappedThisRound: boolean
  /** How many heroes the staged roster would bring in; 0 when nothing is staged. */
  swapsStaged: number
}

export interface NextStep {
  title: string
  detail: string
}

/**
 * The consequence nobody is told about otherwise: `TournamentService`'s
 * `purgeUnlockedEntries` deletes every entry still in draft when an admin puts
 * the tournament live, so an unlocked roster is not a half-finished entry —
 * it is no entry at all.
 */
export const UNLOCKED_ENTRY_WARNING =
  'If you do not lock your roster before the tournament goes live, your entry is removed and you will not score any points.'

function heroes(count: number): string {
  return count === 1 ? '1 hero' : `${count} heroes`
}

function swaps(count: number): string {
  return count === 1 ? '1 swap' : `${count} swaps`
}

/** "1 more hero" / "2 more heroes" — the count reads before the noun, not after. */
function moreHeroes(count: number): string {
  return count === 1 ? '1 more hero' : `${count} more heroes`
}

export function rosterStage(state: RosterState): RosterStage {
  if (!state.registered) return 'REGISTER'
  if (state.locked) return swappable(state) ? 'SWAP' : 'DONE'
  if (state.picked < state.rosterSize || state.remaining < 0) return 'PICK'
  return 'LOCK'
}

/**
 * Whether this entry could still spend something in the window that is open.
 *
 * All three have to hold: a window, an allowance to spend in it, and a
 * submission not already made. Any one of them missing puts a locked roster
 * back at `DONE`, where there genuinely is nothing more to do.
 */
function swappable(state: RosterState): boolean {
  return state.swapWindowOpen && state.swapsAvailable > 0 && !state.alreadySwappedThisRound
}

/** The single reason the lock button is disabled, or `null` when it is live. */
export function lockBlockedReason(state: RosterState): string | null {
  if (!state.registered) return 'Register first to start picking heroes.'
  if (state.locked) return null
  if (state.picked < state.rosterSize) {
    return `Pick ${moreHeroes(state.rosterSize - state.picked)} to complete your roster.`
  }
  if (state.remaining < 0) {
    return `You are ${formatCredits(-state.remaining)} over budget. Swap a hero for a cheaper one.`
  }
  return null
}

/**
 * The single reason the submit-swaps button is disabled, or `null` when it is
 * live — the swap counterpart to [`lockBlockedReason`], and the same contract:
 * it explains exactly the condition that greys the button out.
 *
 * The order matters. "The window is shut" outranks "you have staged nothing",
 * because a manager who cannot act at all should not be told to go and pick.
 */
export function swapBlockedReason(state: RosterState): string | null {
  if (!state.locked) return 'Lock your roster before you can swap heroes.'
  if (!state.swapWindowOpen) return 'The swap window is closed. Wait for the next round.'
  if (state.alreadySwappedThisRound) {
    return 'You have already submitted your swaps for this round.'
  }
  if (state.swapsAvailable < 1) return 'You have no swaps left to spend.'
  if (state.remaining < 0) {
    return `Your new roster is ${formatCredits(-state.remaining)} over budget.`
  }
  if (state.picked !== state.rosterSize) {
    return `A swap is an exchange — your roster has to stay at ${heroes(state.rosterSize)}.`
  }
  if (state.swapsStaged < 1) return 'Choose a hero to bring in, then submit.'
  if (state.swapsStaged > state.swapsAvailable) {
    return `You have staged ${swaps(state.swapsStaged)} but only ${swaps(state.swapsAvailable)} available.`
  }
  return null
}

export function nextStep(state: RosterState): NextStep {
  switch (rosterStage(state)) {
    case 'REGISTER':
      return {
        title: 'Register to start drafting',
        detail: `Registering claims your entry and gives you a ${formatCredits(state.creditGrant)} budget to spend on ${heroes(state.rosterSize)}.`,
      }
    case 'PICK':
      return state.remaining < 0
        ? {
            title: `You are ${formatCredits(-state.remaining)} over budget`,
            detail:
              'Remove a hero, or swap one for a cheaper pick, before you can lock your roster.',
          }
        : {
            title: `Pick ${moreHeroes(state.rosterSize - state.picked)}`,
            detail: `Choose a hero below to add it to your roster. You have ${formatCredits(state.remaining)} left to spend.`,
          }
    case 'LOCK':
      return {
        title: 'Lock in your roster',
        detail: `Your ${heroes(state.rosterSize)} are picked and within budget. Locking is final, but leaving the roster unlocked is worse — an unlocked entry is removed when the tournament goes live.`,
      }
    case 'DONE':
      return {
        title: 'Your roster is locked — nothing more to do',
        detail:
          'Your heroes score points from real match results once the tournament goes live. Follow them on the standings page.',
      }
    case 'SWAP':
      return state.swapsStaged > 0
        ? {
            title: `Submit ${swaps(state.swapsStaged)}`,
            detail:
              'Nothing is sent until you submit, and a round takes one submission — so stage every change you want first.',
          }
        : {
            title: `The swap window is open — you have ${swaps(state.swapsAvailable)}`,
            detail: `Exchange a hero for another that fits your ${formatCredits(state.creditGrant)} budget. Points your heroes have already earned stay with you.`,
          }
  }
}
