import type { HallOfFame } from '@/api/types'

/**
 * Who has won what, for the champion badge on the standings leaderboard.
 *
 * Read off the Hall of Fame rather than added to `StandingsRow` on the server:
 * every `/standings` request — and every refresh the standings stream triggers —
 * would otherwise fold every completed tournament's board as well. A title is a
 * podium row ranked 1, so a shared first place is a title for each manager on
 * it, exactly as the Hall of Fame's own titles table counts it.
 *
 * Maps manager id to the names of the tournaments they won, newest first (the
 * order the Hall of Fame lists tournaments in). A manager with no title is
 * absent rather than mapped to an empty list.
 */
export function championships(hall: HallOfFame | null): Map<number, string[]> {
  const won = new Map<number, string[]>()
  for (const tournament of hall?.tournaments ?? []) {
    for (const row of tournament.podium) {
      if (row.rank !== 1) continue
      const names = won.get(row.managerId)
      if (names) names.push(tournament.name)
      else won.set(row.managerId, [tournament.name])
    }
  }
  return won
}

/** The badge's tooltip and accessible name: which tournaments, not just how many. */
export function championLabel(tournamentNames: string[]): string {
  return `Champion — ${tournamentNames.join(', ')}`
}
