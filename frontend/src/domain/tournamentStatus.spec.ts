import { describe, expect, it } from 'vitest'
import { standingsAvailable } from './tournamentStatus'

describe('standingsAvailable', () => {
  it('offers a board once the tournament is live or finished', () => {
    expect(standingsAvailable('LIVE')).toBe(true)
    expect(standingsAvailable('COMPLETED')).toBe(true)
  })

  it('offers none before the first result can exist', () => {
    expect(standingsAvailable('SCHEDULED')).toBe(false)
    expect(standingsAvailable('REGISTRATION_OPEN')).toBe(false)
  })

  it('treats an unknown tournament as having no board', () => {
    expect(standingsAvailable(null)).toBe(false)
    expect(standingsAvailable(undefined)).toBe(false)
  })
})
