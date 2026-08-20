import { describe, expect, it } from 'vitest'
import { isSafeRedirectPath } from './redirectPath'

describe('isSafeRedirectPath', () => {
  it('accepts an ordinary in-app path', () => {
    expect(isSafeRedirectPath('/tournaments/5/roster')).toBe(true)
  })

  it('accepts a path carrying its own query string', () => {
    expect(isSafeRedirectPath('/standings?tournamentId=5')).toBe(true)
  })

  it('rejects a protocol-relative host escape', () => {
    expect(isSafeRedirectPath('//evil.example.com')).toBe(false)
  })

  it('rejects a backslash host escape', () => {
    expect(isSafeRedirectPath('/\\evil.example.com')).toBe(false)
  })

  it('rejects an absolute URL with its own scheme', () => {
    expect(isSafeRedirectPath('/javascript:alert(1)')).toBe(false)
    expect(isSafeRedirectPath('https://evil.example.com')).toBe(false)
  })

  it('rejects a path with no leading slash', () => {
    expect(isSafeRedirectPath('lobby')).toBe(false)
  })
})
