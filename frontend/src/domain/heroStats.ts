import type { HeroRankRow, HeroStatsBoard } from '@/api/types'

/** The sort/column key of the all-criteria total, alongside each metric's own key. */
export const OVERALL = 'OVERALL'

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

/**
 * One hero across every table at once: its points, its rank and its position in
 * the server's ordering, each keyed by {@link OVERALL} or a metric.
 */
export interface HeroMatrixRow {
  heroId: number
  heroName: string
  cost: number
  points: Record<string, number>
  ranks: Record<string, number>
  /** The row's index in that key's server-ordered table. */
  order: Record<string, number>
}

/**
 * Pivots the per-criterion tables into one row per hero, in the overall order.
 * Every table holds the same hero set — exactly the pool — so this only
 * regroups what the server already ranked; no rank or tie is re-derived here.
 */
export function heroMatrix(board: HeroStatsBoard): HeroMatrixRow[] {
  const byId = new Map<number, HeroMatrixRow>()
  const matrix = board.overall.map((row, index) => {
    const entry: HeroMatrixRow = {
      heroId: row.heroId,
      heroName: row.heroName,
      cost: row.cost,
      points: { [OVERALL]: row.points },
      ranks: { [OVERALL]: row.rank },
      order: { [OVERALL]: index },
    }
    byId.set(row.heroId, entry)
    return entry
  })
  for (const category of board.categories) {
    category.rows.forEach((row, index) => {
      const entry = byId.get(row.heroId)
      if (!entry) return
      entry.points[category.metric] = row.points
      entry.ranks[category.metric] = row.rank
      entry.order[category.metric] = index
    })
  }
  return matrix
}

export type SortDirection = 'asc' | 'desc'

export interface MatrixSort {
  key: string
  direction: SortDirection
}

/**
 * The matrix ordered by one key. Highest first is the server's own order for
 * that table (points, then name), so a tie reads exactly as that table's card
 * does. Lowest first reverses the points but keeps ties in that same order, so
 * flipping the direction never flips a tie. A hero absent from a table sorts last
 * either way.
 */
export function sortMatrix(rows: HeroMatrixRow[], sort: MatrixSort): HeroMatrixRow[] {
  const { key, direction } = sort
  const order = (row: HeroMatrixRow) => row.order[key] ?? Number.POSITIVE_INFINITY
  return [...rows].sort((a, b) => {
    const [oa, ob] = [order(a), order(b)]
    if (oa === ob) return 0
    if (!Number.isFinite(oa) || !Number.isFinite(ob)) return oa < ob ? -1 : 1
    if (direction === 'asc') {
      const byPoints = (a.points[key] ?? 0) - (b.points[key] ?? 0)
      if (byPoints !== 0) return byPoints
    }
    return oa - ob
  })
}

/** A header click: the active column flips its direction, any other starts highest first. */
export function nextSort(current: MatrixSort, clicked: string): MatrixSort {
  if (current.key !== clicked) return { key: clicked, direction: 'desc' }
  return { key: clicked, direction: current.direction === 'desc' ? 'asc' : 'desc' }
}
