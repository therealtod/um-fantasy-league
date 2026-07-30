/**
 * Mirrors the DTOs in `com.umfl.api`.
 *
 * The backend runs Jackson with `default-property-inclusion: non_null`, so a
 * nullable field is *absent* from the payload rather than serialised as `null`.
 * Those are typed optional (`endDate?: string | null`) and every template that
 * shows one needs a `?? '—'` fallback.
 */

export type TournamentStatus = 'SCHEDULED' | 'REGISTRATION_OPEN' | 'LIVE' | 'COMPLETED'
export type TournamentFormat = 'BANQUEST' | 'ARSENAL'
export type EntryStatus = 'DRAFT' | 'LOCKED'
export type HeroSort = 'COST' | 'NAME'

export interface Manager {
  id: number
  handle: string
  displayName: string
  /** Pre-formatted, e.g. "Elite III". */
  rank: string
  isAdmin: boolean
}

export interface Hero {
  id: number
  name: string
  imageUrl: string | null
  /** This tournament's price for the hero — cost is tournament-scoped. */
  cost: number
}

export interface Tournament {
  id: number
  name: string
  format: TournamentFormat
  status: TournamentStatus
  startDate: string
  endDate?: string | null
  capacity: number
  enrolled: number
  rosterSize: number
  /** The budget each registrant is granted for their roster. */
  creditGrant: number
  acceptsRegistration: boolean
  /** Absent, not null, when this manager has no entry — see the note at the top. */
  myEntryStatus?: EntryStatus | null
}

export interface BudgetStatus {
  spent: number
  creditGrant: number
  remaining: number
  utilisation: number
}

export interface Roster {
  entryId: number
  tournamentId: number
  tournamentName: string
  status: EntryStatus
  locked: boolean
  lockedAt: string | null
  rosterSize: number
  heroes: Hero[]
  budget: BudgetStatus
  lockable: boolean
}

/**
 * One leaderboard column. The backend does not know which columns exist until
 * it reads `scoring_coefficient`, so the board carries its own definitions.
 */
export interface MetricColumn {
  metric: string
  label: string
  coefficient: number
}

export interface StandingsRow {
  rank: number
  entryId: number
  managerId: number
  handle: string
  displayName: string
  roster: string[]
  spent: number
  creditGrant: number
  totalPoints: number
  roundPoints: number
  /** Keyed by `MetricColumn.metric`; unknown metrics never appear. */
  breakdown: Record<string, number>
}

export interface StandingsBoard {
  tournamentId: number
  ruleSetName: string
  currentRound: number
  metrics: MetricColumn[]
  rows: StandingsRow[]
}

export interface TickerSide {
  /** Free text. Absent when the result was recorded unattributed — render a fallback. */
  playerLabel?: string
  heroName: string
  healthRemaining: number
  isWinner: boolean
  points: number
}

export interface TickerEntry {
  matchId: number
  round: number
  mapName: string
  playedAt: string
  /** Winner first. A timed draw has no winner at all. */
  sides: TickerSide[]
  bannedHeroNames: string[]
}

export interface RosterViolation {
  rule: string
  message: string
}

/** RFC 7807 body returned by the backend's GlobalExceptionHandler. */
export interface ProblemDetail {
  type?: string
  title?: string
  status?: number
  detail?: string
  violations?: RosterViolation[]
  fields?: Record<string, string>
}

// ---------------------------------------------------------------------------
// Admin types
// ---------------------------------------------------------------------------

export interface CreateTournamentRequest {
  name: string
  format: TournamentFormat
  status: TournamentStatus
  startDate: string // LocalDate as ISO string
  endDate?: string | null
  capacity: number
  rosterSize: number
  creditGrant: number
}

export type UpdateTournamentRequest = CreateTournamentRequest

export interface CreateHeroRequest {
  name: string
  imageUrl?: string | null
}

export type UpdateHeroRequest = CreateHeroRequest

export interface HeroAdminDto {
  id: number
  name: string
  imageUrl: string | null
}

export interface SetHeroCostRequest {
  cost: number
}

export interface CreateMapRequest {
  name: string
}

export type UpdateMapRequest = CreateMapRequest

export interface MapAdminDto {
  id: number
  name: string
}

export interface MatchParticipantRequest {
  /** Who piloted the hero, as free text. Optional — there is no `player` entity to reference. */
  playerLabel?: string | null
  heroId: number
  healthRemaining: number
  isWinner: boolean
}

export interface MatchBanRequest {
  heroId: number
}

export interface RecordMatchRequest {
  round: number
  mapId: number
  playedAt: string // Instant as ISO string
  participants: MatchParticipantRequest[]
  bans: MatchBanRequest[]
}

export interface ParticipantResult {
  participantId: number
  /** Free text. Absent when the result was recorded unattributed. */
  playerLabel?: string
  heroId: number
  heroName: string
  healthRemaining: number
  isWinner: boolean
}

export interface BanResult {
  heroId: number
  heroName: string
}

export interface MatchResultDto {
  matchId: number
  tournamentId: number
  round: number
  mapId: number
  mapName: string
  playedAt: string
  participants: ParticipantResult[]
  bans: BanResult[]
}

export interface ScoringCoefficientRequest {
  metric: string
  coefficient: number
  sortOrder?: number
}

export interface CreateScoringRuleSetRequest {
  name: string
  coefficients: ScoringCoefficientRequest[]
  activate?: boolean
}

export interface UpdateScoringRuleSetRequest {
  name: string
  coefficients: ScoringCoefficientRequest[]
}

export interface ScoringCoefficientDto {
  metric: string
  coefficient: number
  sortOrder: number
}

export interface ScoringRuleSetDto {
  id: number
  tournamentId: number
  name: string
  isActive: boolean
  coefficients: ScoringCoefficientDto[]
  warnings: string[]
}
