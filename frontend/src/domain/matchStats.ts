/**
 * Reading a match's stats sheet: the CSV or JSON file an admin uploads with a
 * result, recording how often each hero attacked, healed, schemed — whatever
 * the tournament's sheet has columns for — in each game. The role a manager
 * gives a hero is what turns those counts into points, server-side.
 *
 * This module only reads the file. Matching its rows onto the games of the form
 * is `matchForm.applyStats`, which knows which heroes played which game.
 *
 * ## Format
 *
 * One row per hero per game. CSV has a header row:
 *
 * ```csv
 * game,hero,ATTACKS,DAMAGE_DEALT,HEALING
 * 1,Bigfoot,5,9,0
 * 1,Beowulf,3,6,2
 * ```
 *
 * `game` is the game number. `hero` names who the row is about; a `side`
 * column (1 or 2, as the form numbers sides) may stand in for it. Every other
 * column is a stat. A blank cell is "not recorded", not zero.
 *
 * JSON is the same rows:
 *
 * ```json
 * [{ "game": 1, "hero": "Bigfoot", "stats": { "ATTACKS": 5, "HEALING": 0 } }]
 * ```
 *
 * Stat names are normalised the server's way plus a little (trimmed, upper
 * case, spaces and hyphens to `_`), so a spreadsheet header of `Damage dealt`
 * lands as `DAMAGE_DEALT`. Values are whole, non-negative counts.
 */
import type { GameStats } from '@/api/types'

export interface StatsRow {
  /** Where the row is in the file — "Line 3" (CSV) or "Row 2" (JSON) — for messages. */
  at: string
  game: number
  /** The hero as the file spells it. Absent when the row names a side instead. */
  hero?: string
  /** 1 or 2, as the form numbers sides. Absent when the row names a hero. */
  side?: number
  stats: GameStats
}

export type ParsedStats = { ok: true; rows: StatsRow[] } | { ok: false; errors: string[] }

/** `Damage dealt` → `DAMAGE_DEALT`. */
export function normaliseStat(raw: string): string {
  return raw.trim().toUpperCase().replace(/[\s-]+/g, '_')
}

/**
 * The same name normalisation as the backend's `name_resolver::normalise` —
 * case, whitespace, the `.` in `Dr. Ellie Sattler`, `&` versus `and` — and
 * deliberately no fuzzier: a near miss that resolved to the wrong hero would
 * score a role bonus for the wrong manager.
 */
export function normaliseHeroName(raw: string): string {
  return raw
    .trim()
    .toLowerCase()
    .replaceAll('&', ' and ')
    .replaceAll('.', ' ')
    .split(/\s+/)
    .filter(Boolean)
    .join(' ')
}

const STAT_NAME = /^[A-Z][A-Z0-9_]*$/

/** Either file shape, decided by name first and content second. */
export function parseStatsFile(text: string, fileName = ''): ParsedStats {
  const trimmed = text.trim()
  if (trimmed === '') return { ok: false, errors: ['The file is empty.'] }
  const looksJson = fileName.toLowerCase().endsWith('.json') || /^[[{]/.test(trimmed)
  return looksJson ? parseJson(trimmed) : parseCsv(trimmed)
}

function parseJson(text: string): ParsedStats {
  let data: unknown
  try {
    data = JSON.parse(text)
  } catch {
    return { ok: false, errors: ['The file is not valid JSON.'] }
  }
  if (!Array.isArray(data)) {
    return { ok: false, errors: ['A JSON stats file is an array of rows: [{ "game": 1, … }].'] }
  }

  const errors: string[] = []
  const rows: StatsRow[] = []
  data.forEach((entry: unknown, index) => {
    const line = index + 1
    if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
      errors.push(`Row ${line}: expected an object.`)
      return
    }
    const record = entry as Record<string, unknown>
    const stats: GameStats = {}
    const rawStats = record.stats
    if (typeof rawStats !== 'object' || rawStats === null || Array.isArray(rawStats)) {
      errors.push(`Row ${line}: "stats" must be an object of stat → count.`)
      return
    }
    for (const [name, value] of Object.entries(rawStats as Record<string, unknown>)) {
      addStat(stats, name, value, `Row ${line}`, errors)
    }
    const row = identify(record.game, record.hero, record.side, stats, `Row ${line}`, errors)
    if (row) rows.push(row)
  })
  return errors.length ? { ok: false, errors } : { ok: true, rows }
}

function parseCsv(text: string): ParsedStats {
  const lines = text.split(/\r?\n/).filter((line) => line.trim() !== '')
  const header = splitCsvLine(lines[0]!).map((cell) => cell.trim())
  const lower = header.map((cell) => cell.toLowerCase())
  const gameAt = lower.indexOf('game')
  const heroAt = lower.indexOf('hero')
  const sideAt = lower.indexOf('side')

  const errors: string[] = []
  if (gameAt === -1) errors.push('The header needs a "game" column.')
  if (heroAt === -1 && sideAt === -1) errors.push('The header needs a "hero" or a "side" column.')
  const statColumns = header
    .map((name, index) => ({ name: normaliseStat(name), index }))
    .filter(({ index }) => index !== gameAt && index !== heroAt && index !== sideAt)
  if (statColumns.length === 0) errors.push('The header names no stat columns.')
  for (const { name } of statColumns) {
    if (!STAT_NAME.test(name)) {
      errors.push(`"${name}" is not a usable stat name — letters, digits and _ only.`)
    }
  }
  const seen = new Set<string>()
  for (const { name } of statColumns) {
    if (seen.has(name)) errors.push(`The stat ${name} is a column twice.`)
    seen.add(name)
  }
  if (errors.length) return { ok: false, errors }

  const rows: StatsRow[] = []
  lines.slice(1).forEach((raw, index) => {
    const line = index + 2
    const cells = splitCsvLine(raw)
    const stats: GameStats = {}
    for (const { name, index: at } of statColumns) {
      const cell = cells[at]?.trim() ?? ''
      if (cell !== '') addStat(stats, name, cell, `Line ${line}`, errors)
    }
    const row = identify(
      cells[gameAt]?.trim(),
      heroAt === -1 ? undefined : cells[heroAt]?.trim() || undefined,
      sideAt === -1 ? undefined : cells[sideAt]?.trim() || undefined,
      stats,
      `Line ${line}`,
      errors,
    )
    if (row) rows.push(row)
  })
  return errors.length ? { ok: false, errors } : { ok: true, rows }
}

/**
 * One CSV line's cells, honouring double-quoted cells (a hero named
 * "Jekyll, Hyde" would need one) and `""` as an escaped quote.
 */
function splitCsvLine(line: string): string[] {
  const cells: string[] = []
  let cell = ''
  let quoted = false
  for (let i = 0; i < line.length; i++) {
    const char = line[i]
    if (quoted) {
      if (char === '"' && line[i + 1] === '"') {
        cell += '"'
        i++
      } else if (char === '"') {
        quoted = false
      } else {
        cell += char
      }
    } else if (char === '"') {
      quoted = true
    } else if (char === ',') {
      cells.push(cell)
      cell = ''
    } else {
      cell += char
    }
  }
  cells.push(cell)
  return cells
}

function addStat(stats: GameStats, rawName: string, rawValue: unknown, at: string, errors: string[]) {
  const name = normaliseStat(rawName)
  if (!STAT_NAME.test(name)) {
    errors.push(`${at}: "${rawName}" is not a usable stat name.`)
    return
  }
  const value = typeof rawValue === 'number' ? rawValue : Number(String(rawValue).trim())
  if (!Number.isInteger(value) || value < 0) {
    errors.push(`${at}: ${name} must be a whole number of 0 or more, not "${String(rawValue)}".`)
    return
  }
  if (name in stats) {
    errors.push(`${at}: ${name} appears twice.`)
    return
  }
  stats[name] = value
}

function identify(
  rawGame: unknown,
  rawHero: unknown,
  rawSide: unknown,
  stats: GameStats,
  at: string,
  errors: string[],
): StatsRow | null {
  const game = Number(rawGame)
  if (!Number.isInteger(game) || game < 1) {
    errors.push(`${at}: "game" must be a game number, not "${String(rawGame ?? '')}".`)
    return null
  }
  const hero = typeof rawHero === 'string' && rawHero.trim() !== '' ? rawHero.trim() : undefined
  let side: number | undefined
  if (rawSide !== undefined && rawSide !== null && rawSide !== '') {
    side = Number(rawSide)
    if (side !== 1 && side !== 2) {
      errors.push(`${at}: "side" must be 1 or 2, not "${String(rawSide)}".`)
      return null
    }
  }
  if (hero === undefined && side === undefined) {
    errors.push(`${at}: name the hero (or the side) the stats belong to.`)
    return null
  }
  return { at, game, hero, side, stats }
}
