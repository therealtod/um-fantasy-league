import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const currentDevManagerId: { value: number | null } = { value: null }
const signOut = vi.fn().mockResolvedValue(undefined)
const push = vi.fn()
const currentRoute = { value: { name: 'roster', fullPath: '/tournaments/1/roster' } }

vi.mock('@/lib/developmentIdentity', () => ({
  get developmentManagerId() {
    return currentDevManagerId.value
  },
  get isDevelopmentIdentityMode() {
    return currentDevManagerId.value !== null
  },
}))

vi.mock('@/lib/supabaseClient', () => ({
  supabase: {
    auth: {
      getSession: vi.fn().mockResolvedValue({ data: { session: null } }),
    },
  },
}))

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ signOut }),
}))

vi.mock('@/router', () => ({
  default: {
    currentRoute,
    push,
  },
}))

const { api, ApiError } = await import('./client')

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

describe('api client — 401 handling', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    currentDevManagerId.value = null
    currentRoute.value = { name: 'roster', fullPath: '/tournaments/1/roster' }
    signOut.mockClear()
    push.mockClear()
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(jsonResponse(401, { status: 401, title: 'Unauthorized' })),
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('signs out and bounces to /login with a redirect back to the current path', async () => {
    await expect(api.me()).rejects.toBeInstanceOf(ApiError)
    expect(signOut).toHaveBeenCalledOnce()
    expect(push).toHaveBeenCalledWith({ name: 'login', query: { redirect: '/tournaments/1/roster' } })
  })

  it('does not redirect again if already on the login screen', async () => {
    currentRoute.value = { name: 'login', fullPath: '/login' }

    await expect(api.me()).rejects.toThrow()
    expect(signOut).toHaveBeenCalledOnce()
    expect(push).not.toHaveBeenCalled()
  })

  it('leaves dev identity mode alone — there is no real session to expire', async () => {
    currentDevManagerId.value = 7

    await expect(api.me()).rejects.toThrow()
    expect(signOut).not.toHaveBeenCalled()
    expect(push).not.toHaveBeenCalled()
  })

  it('leaves a non-401 failure alone', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(jsonResponse(422, { status: 422, title: 'Unprocessable', violations: [] })),
    )

    await expect(api.me()).rejects.toThrow()
    expect(signOut).not.toHaveBeenCalled()
    expect(push).not.toHaveBeenCalled()
  })
})
