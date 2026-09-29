import type { HeroRankRow } from '@/api/types'

/** How many heroes a table shows before its "show all" toggle is opened. */
export const TOP_ROWS = 10

/**
 * The rows a hero table renders: the top {@link TOP_ROWS} unless the table has
 * been expanded. A table with a tie straddling the cut still stops at the
 * limit — the rank column already says the next hero shares that place, and a
 * cut that grew with the tie could balloon to the whole pool on a quiet
 * metric where every hero sits on zero.
 */
export function visibleRows(rows: HeroRankRow[], expanded: boolean, limit = TOP_ROWS): HeroRankRow[] {
  return expanded ? rows : rows.slice(0, limit)
}

/** Whether a table has rows hidden behind its toggle. */
export function hasHiddenRows(rows: HeroRankRow[], limit = TOP_ROWS): boolean {
  return rows.length > limit
}

export type PointsTone = 'muted' | 'penalty' | 'gain'

/**
 * The live-data colour of a point value: nothing earned reads muted, a penalty
 * (a negative value, or any value on a negatively weighted metric) reads as
 * one, and everything else is a gain. The same rule `StandingsView` applies to
 * its breakdown cells.
 */
export function pointsTone(value: number, coefficient = 1): PointsTone {
  if (value === 0) return 'muted'
  if (value < 0 || coefficient < 0) return 'penalty'
  return 'gain'
}
