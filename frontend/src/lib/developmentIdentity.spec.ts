import { describe, expect, it } from 'vitest'
import { configuredDevelopmentManagerId } from './developmentIdentity'

describe('configuredDevelopmentManagerId', () => {
  it('enables a configured local manager in a development build', () => {
    expect(configuredDevelopmentManagerId(true, '3')).toBe(3)
  })

  it('never enables the mode in a production build', () => {
    expect(configuredDevelopmentManagerId(false, '3')).toBeNull()
  })

  it('leaves the mode disabled when no manager is configured', () => {
    expect(configuredDevelopmentManagerId(true, undefined)).toBeNull()
  })

  it('rejects invalid manager identifiers', () => {
    expect(() => configuredDevelopmentManagerId(true, 'not-a-number')).toThrow(
      'VITE_DEV_MANAGER_ID must be a positive integer',
    )
  })
})
