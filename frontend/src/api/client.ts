import { supabase } from '@/lib/supabaseClient'
import { developmentManagerId } from '@/lib/developmentIdentity'
import type {
  CreateHeroRequest,
  CreateMapRequest,
  CreateScoringRuleSetRequest,
  CreateTournamentRequest,
  Hero,
  HeroAdminDto,
  HeroSort,
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
    throw new ApiError(response.status, problem)
  }

  if (response.status === 204) return undefined as T
  return (await response.json()) as T
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

    // Matches
    listMatches: (tournamentId: number, round?: number): Promise<MatchResultDto[]> =>
      request(`/admin/tournaments/${tournamentId}/matches${queryString({ round })}`),

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

    // Scoring
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
