import { describe, expect, it } from 'vitest'
import type { HeroRankRow } from '@/api/types'
import { TOP_ROWS, hasHiddenRows, pointsTone, visibleRows } from './heroStats'

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
