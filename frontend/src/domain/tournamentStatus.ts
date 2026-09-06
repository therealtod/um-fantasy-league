import type { Tournament, TournamentStatus } from '@/api/types'
import { swapAllowance } from '@/domain/rosterPolicy'

/**
 * Which tournaments have a standings board to look at.
 *
 * A board is only worth showing once results can exist, so `StandingsView`
 * offers `LIVE` and `COMPLETED` tournaments and nothing else. That rule is
 * shared rather than repeated because the roster page has to state the same
 * thing in words — telling a manager to go watch standings that the standings
 * page will not let them select is the drift this exists to stop.
 */
export function standingsAvailable(status: TournamentStatus | null | undefined): boolean {
  return status === 'LIVE' || status === 'COMPLETED'
}

/**
 * Whether the lobby should send this manager to the roster page to swap.
 *
 * Deliberately **optimistic**, and it has to be: the swap allowance a manager
 * has actually got left is per-entry (`Roster.swapsAvailable`) and only reaches
 * the client on the roster payload, so a tournament list cannot know whether
 * this manager has already spent the window. Answering the weaker question —
 * "could this window pay anybody anything" — is the most the lobby can honestly
 * do, and the roster page names the exact reason on arrival via
 * `swapBlockedReason` if it turns out there is nothing left to spend. A button
 * that over-offers by one click beats one that hides the mechanic entirely,
 * which is what a lobby with no swap route at all was doing.
 *
 * `swapAllowance` with no spend folds in the two states that are definitively
 * dead for everyone: `swapsPerRound` of 0 switches the mechanic off, and round 1
 * has had no window elapse yet.
 *
 * The `COMPLETED` exclusion mirrors the backend's `Tournament::accepts_swaps`,
 * which ANDs the flag with "not completed" but is not serialised — the wire
 * carries the raw `swapWindowOpen`, so this side has to re-derive it rather than
 * trust a window left open on a tournament that has since finished.
 */
export function swapInviteOpen(tournament: Tournament): boolean {
  return (
    tournament.myEntryStatus === 'LOCKED' &&
    tournament.swapWindowOpen &&
    tournament.status !== 'COMPLETED' &&
    swapAllowance(tournament.swapsPerRound, tournament.currentRound, 0) > 0
  )
}
