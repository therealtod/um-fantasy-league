import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Tournament } from '@/api/types'

vi.mock('@/api/client', () => ({
  api: { tournaments: vi.fn(), register: vi.fn() },
  ApiError: class extends Error {},
  describeError: (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback),
}))

import { useTournamentsStore } from './tournaments'

/**
 * The backend serialises with `default-property-inclusion: non_null`, so a
 * tournament the manager has not entered arrives with **no** `myEntryStatus`
 * key at all — not `myEntryStatus: null`. These fixtures are shaped like the
 * wire, deliberately: comparing against `null` here would pass while the app
 * silently showed nobody a Register button.
 */
const open: Tournament = {
  id: 1,
  name: 'Winter of Champions',
  format: 'ARSENAL',
  status: 'REGISTRATION_OPEN',
  startDate: '2026-08-14',
  capacity: 64,
  enrolled: 0,
  rosterSize: 3,
  creditGrant: 10_000,
  acceptsRegistration: true,
  // myEntryStatus deliberately absent
}

const entered: Tournament = {
  ...open,
  id: 2,
  name: 'Summer of Legends',
  status: 'COMPLETED',
  acceptsRegistration: false,
  myEntryStatus: 'LOCKED',
}

const closedAndUnentered: Tournament = {
  ...open,
  id: 3,
  name: 'Spring of Myths',
  status: 'SCHEDULED',
  acceptsRegistration: false,
  // myEntryStatus deliberately absent
}

describe('tournaments store', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('offers registration for an open tournament the manager has not entered', () => {
    const store = useTournamentsStore()
    store.tournaments = [open, entered, closedAndUnentered]

    expect(store.openForRegistration.map((t) => t.id)).toEqual([1])
  })

  it('lets the manager draft where they hold an entry or can still register', () => {
    const store = useTournamentsStore()
    store.tournaments = [open, entered, closedAndUnentered]

    expect(store.draftable.map((t) => t.id)).toEqual([1, 2])
  })

  it('stops offering registration once an entry exists', () => {
    const store = useTournamentsStore()
    store.tournaments = [{ ...open, myEntryStatus: 'DRAFT' }]

    expect(store.openForRegistration).toEqual([])
    expect(store.draftable.map((t) => t.id)).toEqual([1])
  })
})
