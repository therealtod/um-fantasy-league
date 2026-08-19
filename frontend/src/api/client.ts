import { supabase } from '@/lib/supabaseClient'
import { developmentManagerId, isDevelopmentIdentityMode } from '@/lib/developmentIdentity'
import router from '@/router'
import { useAuthStore } from '@/stores/auth'
import type {
  CreateHeroRequest,
  CreateMapRequest,
  CreateScoringRuleSetRequest,
  CreateTournamentRequest,
  Hero,
  HeroAdminDto,
  HeroPoolEntryRequest,
  HeroSort,
  ImportMatchRequest,
  MatchImportPreviewDto,
  Manager,
  MapAdminDto,
  MatchResultDto,
  ProblemDetail,
  RecordMatchRequest,
  Roster,
  RosterViolation,
  ScoringRuleSetDto,
  StandingsBoard,
  TickerEntry,
  Tournament,
  TournamentStatus,
  UpdateHeroRequest,
  UpdateMapRequest,
  UpdateScoringRuleSetRequest,
  UpdateTournamentRequest,
} from './types'

/** A non-2xx response, carrying the backend's RFC 7807 body. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly problem: ProblemDetail,
  ) {
    super(problem.detail ?? problem.title ?? `Request failed with status ${status}`)
    this.name = 'ApiError'
  }

  /** Roster rule breaches, present on 422 responses. */
  get violations(): RosterViolation[] {
    return this.problem.violations ?? []
  }
}

/**
 * The same identity resolution every request needs: a dev `X-Manager-Id`
 * header, or a Supabase bearer token. Exported so `sseClient.ts` can attach
 * identical headers to its hand-rolled `fetch`-based stream — a native
 * `EventSource` can't set an `Authorization` header, which is exactly why that
 * client is built on `fetch` instead.
 */
export async function buildAuthHeaders(): Promise<HeadersInit> {
  const session = developmentManagerId === null
    ? (await supabase.auth.getSession()).data.session
    : null

  return {
    ...(developmentManagerId !== null ? { 'X-Manager-Id': String(developmentManagerId) } : {}),
    ...(session ? { Authorization: `Bearer ${session.access_token}` } : {}),
  }
}

/**
 * A 401 means the session Supabase handed us no longer verifies — expired,
 * revoked, whatever — not that this particular request lacked a header.
 * Without central handling, every store surfaces that as its own "request
 * failed" text on whatever screen the manager happened to be looking at, with
 * no path back to signing in again. This clears the stale session and bounces
 * once to `/login`, carrying the current path as `redirect`. Dev identity
 * mode has no real session to expire — `isAuthenticated` is hardwired true
 * there (see `useAuthStore`), so redirecting would just bounce straight back.
 */
async function handleUnauthorized(): Promise<void> {
  if (isDevelopmentIdentityMode) return

  await useAuthStore().signOut()
  if (router.currentRoute.value.name !== 'login') {
    void router.push({ name: 'login', query: { redirect: router.currentRoute.value.fullPath } })
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api${path}`, {
    ...init,
    headers: {
      Accept: 'application/json',
      ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
      ...(await buildAuthHeaders()),
      ...init?.headers,
    },
  })

  if (!response.ok) {
    let problem: ProblemDetail = { status: response.status, title: response.statusText }
    try {
      problem = { ...problem, ...(await response.json()) }
    } catch {
      // A non-JSON error body (a proxy error page, say) leaves the defaults in place.
    }
    if (response.status === 401) {
      await handleUnauthorized()
    }
    throw new ApiError(response.status, problem)
  }

  if (response.status === 204) return undefined as T
  return (await response.json()) as T
}

/**
 * Renders a caught value to a display string, falling back to `fallback` for
 * anything that isn't an `Error` (a thrown string, a rejected non-Error, …).
 * Centralises the `e instanceof Error ? e.message : fallback` pattern that
 * every store/wizard catch block otherwise repeats inline.
 */
export function describeError(e: unknown, fallback: string): string {
  return e instanceof Error ? e.message : fallback
}

/**
 * Rule violations from a 422 `ApiError`, as display strings — `[]` for
 * anything else. `ApiError.violations` already covers roster, match and
 * scoring rule breaches alike (`GlobalExceptionHandler` serialises all three
 * `*RuleException` types to the same `{rule, message}` shape), so this is the
 * one place that reaches past `describeError`'s single joined sentence to the
 * structured list underneath it.
 */
export function violationMessages(e: unknown): string[] {
  return e instanceof ApiError ? e.violations.map((violation) => violation.message) : []
}

function queryString(params: Record<string, string | number | undefined | null>): string {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== '') {
      search.set(key, String(value))
    }
  }
  const rendered = search.toString()
  return rendered ? `?${rendered}` : ''
}

export const api = {
  me: (): Promise<Manager> => request('/me'),

  tournaments: (status?: TournamentStatus): Promise<Tournament[]> =>
    request(`/tournaments${queryString({ status })}`),

  /** The hero pool is tournament-scoped because cost is set per tournament. */
  heroes: (
    tournamentId: number,
    params: { search?: string; sort?: HeroSort } = {},
  ): Promise<Hero[]> => request(`/tournaments/${tournamentId}/heroes${queryString(params)}`),

  register: (tournamentId: number): Promise<Roster> =>
    request(`/tournaments/${tournamentId}/entries`, { method: 'POST' }),

  myRoster: (tournamentId: number): Promise<Roster> =>
    request(`/tournaments/${tournamentId}/entries/me`),

  setSlots: (tournamentId: number, heroIds: number[]): Promise<Roster> =>
    request(`/tournaments/${tournamentId}/entries/me/slots`, {
      method: 'PUT',
      body: JSON.stringify({ heroIds }),
    }),

  lockRoster: (tournamentId: number): Promise<Roster> =>
    request(`/tournaments/${tournamentId}/entries/me/lock`, { method: 'POST' }),

  standings: (tournamentId: number): Promise<StandingsBoard> =>
    request(`/tournaments/${tournamentId}/standings`),

  matches: (tournamentId: number, sinceMatchId = 0, limit = 25): Promise<TickerEntry[]> =>
    request(`/tournaments/${tournamentId}/matches${queryString({ sinceMatchId, limit })}`),

  admin: {
    // Tournaments
    createTournament: (data: CreateTournamentRequest): Promise<Tournament> =>
      request('/admin/tournaments', { method: 'POST', body: JSON.stringify(data) }),

    updateTournament: (id: number, data: UpdateTournamentRequest): Promise<Tournament> =>
      request(`/admin/tournaments/${id}`, { method: 'PUT', body: JSON.stringify(data) }),

    deleteTournament: (id: number): Promise<void> =>
      request(`/admin/tournaments/${id}`, { method: 'DELETE' }),

    // Heroes
    listHeroes: (): Promise<HeroAdminDto[]> =>
      request('/admin/heroes'),

    createHero: (data: CreateHeroRequest): Promise<HeroAdminDto> =>
      request('/admin/heroes', { method: 'POST', body: JSON.stringify(data) }),

    updateHero: (id: number, data: UpdateHeroRequest): Promise<HeroAdminDto> =>
      request(`/admin/heroes/${id}`, { method: 'PUT', body: JSON.stringify(data) }),

    setHeroCost: (tournamentId: number, heroId: number, cost: number): Promise<Hero> =>
      request(`/admin/tournaments/${tournamentId}/heroes/${heroId}`, {
        method: 'PUT',
        body: JSON.stringify({ cost }),
      }),

    listHeroPool: (tournamentId: number): Promise<Hero[]> =>
      request(`/admin/tournaments/${tournamentId}/heroes`),

    /** Adds/re-prices several heroes in one request — the batch counterpart to `setHeroCost`. */
    addHeroesToPool: (tournamentId: number, heroes: HeroPoolEntryRequest[]): Promise<Hero[]> =>
      request(`/admin/tournaments/${tournamentId}/heroes`, {
        method: 'POST',
        body: JSON.stringify({ heroes }),
      }),

    removeHeroFromPool: (tournamentId: number, heroId: number): Promise<void> =>
      request(`/admin/tournaments/${tournamentId}/heroes/${heroId}`, { method: 'DELETE' }),

    // Maps
    listMaps: (): Promise<MapAdminDto[]> =>
      request('/admin/maps'),

    createMap: (data: CreateMapRequest): Promise<MapAdminDto> =>
      request('/admin/maps', { method: 'POST', body: JSON.stringify(data) }),

    updateMap: (id: number, data: UpdateMapRequest): Promise<MapAdminDto> =>
      request(`/admin/maps/${id}`, { method: 'PUT', body: JSON.stringify(data) }),

    addMapToPool: (tournamentId: number, mapId: number): Promise<MapAdminDto> =>
      request(`/admin/tournaments/${tournamentId}/maps/${mapId}`, { method: 'PUT' }),

    listMapPool: (tournamentId: number): Promise<MapAdminDto[]> =>
      request(`/admin/tournaments/${tournamentId}/maps`),

    /** Adds several maps in one request — the batch counterpart to `addMapToPool`. */
    addMapsToPool: (tournamentId: number, mapIds: number[]): Promise<MapAdminDto[]> =>
      request(`/admin/tournaments/${tournamentId}/maps`, {
        method: 'POST',
        body: JSON.stringify({ mapIds }),
      }),

    removeMapFromPool: (tournamentId: number, mapId: number): Promise<void> =>
      request(`/admin/tournaments/${tournamentId}/maps/${mapId}`, { method: 'DELETE' }),

    // Matches
    // Newest first, and bounded server-side (200 by default, 500 max) — pass `limit` to widen it.
    listMatches: (tournamentId: number, round?: number, limit?: number): Promise<MatchResultDto[]> =>
      request(`/admin/tournaments/${tournamentId}/matches${queryString({ round, limit })}`),

    getMatch: (tournamentId: number, matchId: number): Promise<MatchResultDto> =>
      request(`/admin/tournaments/${tournamentId}/matches/${matchId}`),

    recordMatch: (tournamentId: number, data: RecordMatchRequest): Promise<MatchResultDto> =>
      request(`/admin/tournaments/${tournamentId}/matches`, {
        method: 'POST',
        body: JSON.stringify(data),
      }),

    correctMatch: (
      tournamentId: number,
      matchId: number,
      data: RecordMatchRequest,
    ): Promise<MatchResultDto> =>
      request(`/admin/tournaments/${tournamentId}/matches/${matchId}`, {
        method: 'PUT',
        body: JSON.stringify(data),
      }),

    deleteMatch: (tournamentId: number, matchId: number): Promise<void> =>
      request(`/admin/tournaments/${tournamentId}/matches/${matchId}`, { method: 'DELETE' }),

    /**
     * Scrapes a Tabletop League match URL and resolves it against this
     * tournament's catalogue. Writes nothing — the returned draft is submitted
     * through `recordMatch` once the admin has reviewed it and supplied the
     * round number the source page doesn't carry.
     *
     * Slow by the standards of every other call here (a real browser renders the
     * page), so callers should show a loading state rather than assuming it
     * returns promptly.
     */
    importMatch: (tournamentId: number, sourceUrl: string): Promise<MatchImportPreviewDto> =>
      request(`/admin/tournaments/${tournamentId}/matches/import`, {
        method: 'POST',
        body: JSON.stringify({ sourceUrl } satisfies ImportMatchRequest),
      }),

    // Scoring
    listScoringRuleSets: (tournamentId: number): Promise<ScoringRuleSetDto[]> =>
      request(`/admin/tournaments/${tournamentId}/scoring-rule-sets`),

    createScoringRuleSet: (
      tournamentId: number,
      data: CreateScoringRuleSetRequest,
    ): Promise<ScoringRuleSetDto> =>
      request(`/admin/tournaments/${tournamentId}/scoring-rule-sets`, {
        method: 'POST',
        body: JSON.stringify(data),
      }),

    updateScoringRuleSet: (
      tournamentId: number,
      ruleSetId: number,
      data: UpdateScoringRuleSetRequest,
    ): Promise<ScoringRuleSetDto> =>
      request(`/admin/tournaments/${tournamentId}/scoring-rule-sets/${ruleSetId}`, {
        method: 'PUT',
        body: JSON.stringify(data),
      }),

    activateScoringRuleSet: (tournamentId: number, ruleSetId: number): Promise<ScoringRuleSetDto> =>
      request(`/admin/tournaments/${tournamentId}/scoring-rule-sets/${ruleSetId}/activate`, {
        method: 'POST',
      }),
  },
}
