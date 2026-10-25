import { describe, expect, it } from 'vitest'
import type { HallOfFame, HallOfFameEntry, StandingsRow } from '@/api/types'
import { championLabel, championships } from './champions'

function row(rank: number, managerId: number): StandingsRow {
  return {
    rank,
    entryId: managerId * 10,
    managerId,
    handle: `M${managerId}`,
    displayName: `Manager ${managerId}`,
    roster: [],
    spent: 0,
    creditGrant: 10000,
    totalPoints: 0,
    roundPoints: 0,
    breakdown: {},
  }
}

function tournament(tournamentId: number, name: string, podium: StandingsRow[]): HallOfFameEntry {
  return {
    tournamentId,
    name,
    format: 'BANQUEST',
    startDate: '2026-01-01',
    ruleSetName: 'Standard',
    entrants: podium.length,
    podium,
  }
}

const hall = (...tournaments: HallOfFameEntry[]): HallOfFame => ({ titles: [], tournaments })

describe('championships', () => {
  it('credits the manager ranked first with the tournament', () => {
    const won = championships(hall(tournament(1, 'Spring Open', [row(1, 7), row(2, 8), row(3, 9)])))

    expect(won.get(7)).toEqual(['Spring Open'])
  })

  it('gives nothing to the rest of the podium', () => {
    const won = championships(hall(tournament(1, 'Spring Open', [row(1, 7), row(2, 8), row(3, 9)])))

    expect(won.has(8)).toBe(false)
    expect(won.has(9)).toBe(false)
    expect(won.size).toBe(1)
  })

  it('makes a shared first place a title for each manager on it', () => {
    const won = championships(hall(tournament(1, 'Spring Open', [row(1, 7), row(1, 8), row(3, 9)])))

    expect(won.get(7)).toEqual(['Spring Open'])
    expect(won.get(8)).toEqual(['Spring Open'])
  })

  it('accumulates titles across tournaments, newest first', () => {
    const won = championships(
      hall(
        tournament(3, 'Autumn Finals', [row(1, 7)]),
        tournament(2, 'Summer Clash', [row(1, 8), row(2, 7)]),
        tournament(1, 'Spring Open', [row(1, 7)]),
      ),
    )

    expect(won.get(7)).toEqual(['Autumn Finals', 'Spring Open'])
    expect(won.get(8)).toEqual(['Summer Clash'])
  })

  it('crowns nobody for a tournament nobody entered, or before the Hall of Fame loads', () => {
    expect(championships(hall(tournament(1, 'Spring Open', []))).size).toBe(0)
    expect(championships(null).size).toBe(0)
  })
})

describe('championLabel', () => {
  it('names every tournament won', () => {
    expect(championLabel(['Autumn Finals', 'Spring Open'])).toBe(
      'Champion — Autumn Finals, Spring Open',
    )
  })
})
