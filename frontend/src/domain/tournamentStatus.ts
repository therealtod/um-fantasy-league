import type { TournamentStatus } from '@/api/types'

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
