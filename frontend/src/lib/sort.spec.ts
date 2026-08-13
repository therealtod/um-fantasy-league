import { describe, expect, it } from 'vitest'
import { byName } from './sort'

describe('byName', () => {
  it('sorts alphabetically', () => {
    const items = [{ name: 'Sinbad' }, { name: 'Achilles' }, { name: 'Medusa' }]
    expect(items.sort(byName).map((i) => i.name)).toEqual(['Achilles', 'Medusa', 'Sinbad'])
  })

  it('ignores case', () => {
    const items = [{ name: 'medusa' }, { name: 'Alice' }, { name: 'king Arthur' }]
    expect(items.sort(byName).map((i) => i.name)).toEqual(['Alice', 'king Arthur', 'medusa'])
  })
})
