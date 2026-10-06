import { describe, expect, it } from 'vitest'
import { normaliseHeroName, normaliseStat, parseStatsFile, type ParsedStats } from './matchStats'

function rows(parsed: ParsedStats) {
  if (!parsed.ok) throw new Error(`expected a parse, got ${parsed.errors.join('; ')}`)
  return parsed.rows
}

function errors(parsed: ParsedStats) {
  if (parsed.ok) throw new Error('expected errors, got a parse')
  return parsed.errors
}

describe('parseStatsFile — CSV', () => {
  it('reads one row per hero per game, every other column a stat', () => {
    const parsed = parseStatsFile(
      'game,hero,ATTACKS,DAMAGE_DEALT,HEALING\n1,Bigfoot,5,9,0\n1,Beowulf,3,6,2\n',
      'stats.csv',
    )

    expect(rows(parsed)).toEqual([
      { at: 'Line 2', game: 1, hero: 'Bigfoot', side: undefined, stats: { ATTACKS: 5, DAMAGE_DEALT: 9, HEALING: 0 } },
      { at: 'Line 3', game: 1, hero: 'Beowulf', side: undefined, stats: { ATTACKS: 3, DAMAGE_DEALT: 6, HEALING: 2 } },
    ])
  })

  it('normalises spreadsheet-style headers into stat names', () => {
    const parsed = parseStatsFile('Game,Hero,Damage dealt,schemes-played\n1,Alice,4,1')

    expect(rows(parsed)[0]!.stats).toEqual({ DAMAGE_DEALT: 4, SCHEMES_PLAYED: 1 })
  })

  it('reads a blank cell as not recorded rather than zero', () => {
    expect(rows(parseStatsFile('game,hero,ATTACKS,HEALING\n1,Alice,4,'))[0]!.stats).toEqual({ ATTACKS: 4 })
  })

  it('takes a side in place of a hero, numbered 1 and 2 as the form shows them', () => {
    const parsed = rows(parseStatsFile('game,side,ATTACKS\n1,2,4'))

    expect(parsed[0]).toMatchObject({ game: 1, side: 2, hero: undefined })
  })

  it('honours a quoted cell with a comma in it, and CRLF line endings', () => {
    const parsed = rows(parseStatsFile('game,hero,ATTACKS\r\n1,"Holmes, Sherlock",4\r\n'))

    expect(parsed[0]!.hero).toBe('Holmes, Sherlock')
  })

  it('names a header with no game, no hero or side, or no stats', () => {
    expect(errors(parseStatsFile('round,player\n1,x'))).toEqual([
      'The header needs a "game" column.',
      'The header needs a "hero" or a "side" column.',
    ])
    expect(errors(parseStatsFile('game,hero\n1,Alice'))).toEqual(['The header names no stat columns.'])
  })

  it('refuses a stat column that could never be stored, or appears twice', () => {
    expect(errors(parseStatsFile('game,hero,1st blood\n1,Alice,1'))).toEqual([
      '"1ST_BLOOD" is not a usable stat name — letters, digits and _ only.',
    ])
    expect(errors(parseStatsFile('game,hero,attacks,ATTACKS\n1,Alice,1,2'))).toEqual([
      'The stat ATTACKS is a column twice.',
    ])
  })

  it('reports every bad row at once, by line', () => {
    const problems = errors(parseStatsFile('game,hero,ATTACKS\nx,Alice,1\n2,Bob,-3\n3,,1\n4,Cy,1.5'))

    expect(problems).toEqual([
      'Line 2: "game" must be a game number, not "x".',
      'Line 3: ATTACKS must be a whole number of 0 or more, not "-3".',
      'Line 4: name the hero (or the side) the stats belong to.',
      'Line 5: ATTACKS must be a whole number of 0 or more, not "1.5".',
    ])
  })

  it('refuses a side that is not 1 or 2', () => {
    expect(errors(parseStatsFile('game,side,ATTACKS\n1,0,4'))).toEqual([
      'Line 2: "side" must be 1 or 2, not "0".',
    ])
  })
})

describe('parseStatsFile — JSON', () => {
  it('reads the same rows from an array', () => {
    const parsed = parseStatsFile(
      JSON.stringify([{ game: 2, hero: 'Medusa', stats: { attacks: 3, 'Healing done': 1 } }]),
      'stats.json',
    )

    expect(rows(parsed)).toEqual([
      { at: 'Row 1', game: 2, hero: 'Medusa', side: undefined, stats: { ATTACKS: 3, HEALING_DONE: 1 } },
    ])
  })

  it('is recognised by its content even without a .json name', () => {
    expect(rows(parseStatsFile('[{"game":1,"side":1,"stats":{"ATTACKS":2}}]'))[0]!.side).toBe(1)
  })

  it('names malformed JSON, the wrong top-level shape, and bad rows', () => {
    expect(errors(parseStatsFile('[{', 'x.json'))).toEqual(['The file is not valid JSON.'])
    expect(errors(parseStatsFile('{"game": 1}'))).toEqual([
      'A JSON stats file is an array of rows: [{ "game": 1, … }].',
    ])
    expect(errors(parseStatsFile('[{"game":1,"hero":"A","stats":[1]}, 3]'))).toEqual([
      'Row 1: "stats" must be an object of stat → count.',
      'Row 2: expected an object.',
    ])
  })
})

describe('parseStatsFile — either', () => {
  it('names an empty file', () => {
    expect(errors(parseStatsFile('   \n'))).toEqual(['The file is empty.'])
  })
})

describe('normalisation', () => {
  it('turns a header into a stat name', () => {
    expect(normaliseStat('  damage - dealt ')).toBe('DAMAGE_DEALT')
  })

  it('matches hero names the way the import does, and no fuzzier', () => {
    expect(normaliseHeroName(' Dr. Ellie  Sattler ')).toBe(normaliseHeroName('dr ellie sattler'))
    expect(normaliseHeroName('Jekyll & Hyde')).toBe(normaliseHeroName('Jekyll and Hyde'))
    expect(normaliseHeroName('Sherlock')).not.toBe(normaliseHeroName('Sherlock Holmes'))
  })
})
