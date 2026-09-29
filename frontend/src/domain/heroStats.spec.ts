import { describe, expect, it } from 'vitest'
import type { HeroRankRow, HeroStatsBoard } from '@/api/types'
import {
  OVERALL,
  TOP_ROWS,
  hasHiddenRows,
  heroMatrix,
  nextSort,
  pointsTone,
  sortMatrix,
  visibleRows,
} from './heroStats'

function rows(count: number): HeroRankRow[] {
  return Array.from({ length: count }, (_, i) => ({
    rank: i + 1,
    heroId: i + 1,
    heroName: `Hero ${i + 1}`,
    cost: 2000,
    points: count - i,
  }))
}

describe('visibleRows', () => {
  it('shows the top ten of a longer table until it is expanded', () => {
    const table = rows(25)
    expect(visibleRows(table, false)).toHaveLength(TOP_ROWS)
    expect(visibleRows(table, false)[0]?.heroName).toBe('Hero 1')
    expect(visibleRows(table, true)).toHaveLength(25)
  })

  it('shows a short table whole either way', () => {
    expect(visibleRows(rows(4), false)).toHaveLength(4)
    expect(hasHiddenRows(rows(4))).toBe(false)
    expect(hasHiddenRows(rows(TOP_ROWS))).toBe(false)
    expect(hasHiddenRows(rows(TOP_ROWS + 1))).toBe(true)
  })

  it('stops at the limit even inside a tie', () => {
    const allZero = rows(30).map((r) => ({ ...r, rank: 1, points: 0 }))
    expect(visibleRows(allZero, false)).toHaveLength(TOP_ROWS)
  })
})

describe('pointsTone', () => {
  it('reads nothing earned as muted', () => {
    expect(pointsTone(0)).toBe('muted')
    expect(pointsTone(0, -6)).toBe('muted')
  })

  it('reads a negative value, or a penalty metric, as a penalty', () => {
    expect(pointsTone(-5.5)).toBe('penalty')
    expect(pointsTone(-12, -6)).toBe('penalty')
  })

  it('reads a positive value on a positive metric as a gain', () => {
    expect(pointsTone(10)).toBe('gain')
    expect(pointsTone(19.25, 1)).toBe('gain')
  })
})

function ranked(entries: [number, string, number, number][]): HeroRankRow[] {
  return entries.map(([heroId, heroName, points, rank]) => ({ rank, heroId, heroName, cost: heroId * 1000, points }))
}

/** Three heroes, server-ordered per table: points desc, ties by name. */
function board(): HeroStatsBoard {
  return {
    tournamentId: 1,
    ruleSetName: 'Standard',
    currentRound: 2,
    overall: ranked([
      [1, 'Alice', 30, 1],
      [2, 'Bigfoot', 20, 2],
      [3, 'Medusa', 10, 3],
    ]),
    categories: [
      {
        metric: 'WIN',
        label: 'Win',
        coefficient: 10,
        rows: ranked([
          [3, 'Medusa', 20, 1],
          [1, 'Alice', 10, 2],
          [2, 'Bigfoot', 10, 2],
        ]),
      },
      {
        metric: 'LOSS',
        label: 'Loss',
        coefficient: -5,
        rows: ranked([
          [2, 'Bigfoot', 0, 1],
          [1, 'Alice', -5, 2],
          [3, 'Medusa', -10, 3],
        ]),
      },
    ],
  }
}

const names = (rows: { heroName: string }[]) => rows.map((r) => r.heroName)

describe('heroMatrix', () => {
  it('gives every hero one row, in the overall order', () => {
    expect(names(heroMatrix(board()))).toEqual(['Alice', 'Bigfoot', 'Medusa'])
  })

  it('pivots each table onto its hero by id, keeping the server rank', () => {
    const medusa = heroMatrix(board()).find((r) => r.heroId === 3)!
    expect(medusa.cost).toBe(3000)
    expect(medusa.points).toEqual({ [OVERALL]: 10, WIN: 20, LOSS: -10 })
    expect(medusa.ranks).toEqual({ [OVERALL]: 3, WIN: 1, LOSS: 3 })
    expect(medusa.order).toEqual({ [OVERALL]: 2, WIN: 0, LOSS: 2 })
  })
})

describe('sortMatrix', () => {
  it('highest first is exactly the server order of that table', () => {
    const matrix = heroMatrix(board())
    expect(names(sortMatrix(matrix, { key: 'WIN', direction: 'desc' }))).toEqual(['Medusa', 'Alice', 'Bigfoot'])
    expect(names(sortMatrix(matrix, { key: 'LOSS', direction: 'desc' }))).toEqual(['Bigfoot', 'Alice', 'Medusa'])
  })

  it('lowest first reverses the points but never flips a tie', () => {
    const matrix = heroMatrix(board())
    expect(names(sortMatrix(matrix, { key: 'WIN', direction: 'asc' }))).toEqual(['Alice', 'Bigfoot', 'Medusa'])
    expect(names(sortMatrix(matrix, { key: OVERALL, direction: 'asc' }))).toEqual(['Medusa', 'Bigfoot', 'Alice'])
  })

  it('sorts a hero missing from a table last either way', () => {
    const b = board()
    b.categories[0]!.rows = b.categories[0]!.rows.filter((r) => r.heroId !== 3)
    const matrix = heroMatrix(b)
    expect(names(sortMatrix(matrix, { key: 'WIN', direction: 'desc' })).at(-1)).toBe('Medusa')
    expect(names(sortMatrix(matrix, { key: 'WIN', direction: 'asc' })).at(-1)).toBe('Medusa')
  })

  it('leaves its input alone', () => {
    const matrix = heroMatrix(board())
    sortMatrix(matrix, { key: 'WIN', direction: 'asc' })
    expect(names(matrix)).toEqual(['Alice', 'Bigfoot', 'Medusa'])
  })
})

describe('nextSort', () => {
  it('starts a new column highest first', () => {
    expect(nextSort({ key: OVERALL, direction: 'asc' }, 'WIN')).toEqual({ key: 'WIN', direction: 'desc' })
  })

  it('flips the active column', () => {
    expect(nextSort({ key: 'WIN', direction: 'desc' }, 'WIN')).toEqual({ key: 'WIN', direction: 'asc' })
    expect(nextSort({ key: 'WIN', direction: 'asc' }, 'WIN')).toEqual({ key: 'WIN', direction: 'desc' })
  })
})
