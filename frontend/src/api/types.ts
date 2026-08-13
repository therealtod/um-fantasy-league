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

export interface TickerGameSide {
  /** Free text. Absent when the result was recorded unattributed — render a fallback. */
  playerLabel?: string
  heroName: string
  healthRemaining: number
  isWinner: boolean
  /** This hero's net score for this game. May be negative. */
  points: number
}

export interface TickerGame {
  gameNumber: number
  mapName: string
  /** Winner first — every game has exactly one, so this is always winner then loser. */
  sides: TickerGameSide[]
}

export interface TickerEntry {
  matchId: number
  round: number
  playedAt: string
  /** Absent when the match has no external link. */
  externalLink?: string
  /** Ordered by game number — one entry per game played in the series. */
  games: TickerGame[]
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

export interface HeroPoolEntryRequest {
  heroId: number
  cost: number
}

export interface AddHeroesToPoolRequest {
  heroes: HeroPoolEntryRequest[]
}

export interface CreateMapRequest {
  name: string
}

export type UpdateMapRequest = CreateMapRequest

export interface MapAdminDto {
  id: number
  name: string
}

export interface AddMapsToPoolRequest {
  mapIds: number[]
}

export type BanType = 'PRE_BAN' | 'OPPONENT_BAN' | 'SELF_BAN'

export interface MatchParticipantRequest {
  /** Who piloted this side for the whole series, as free text. Optional — there is no `player` entity to reference. */
  playerLabel?: string | null
}

export interface MatchGameParticipantRequest {
  heroId: number
  healthRemaining: number
  isWinner: boolean
}

export interface MatchGameRequest {
  gameNumber: number
  mapId: number
  participants: MatchGameParticipantRequest[]
}

export interface MatchBanRequest {
  heroId: number
  banType: BanType
}

export interface RecordMatchRequest {
  round: number
  playedAt: string // Instant as ISO string
  externalLink?: string | null
  participants: MatchParticipantRequest[]
  games: MatchGameRequest[]
  bans: MatchBanRequest[]
}

export interface MatchParticipantResult {
  /** 0 or 1 — a stable ordinal for the whole series. */
  side: number
  /** Free text. Absent when the result was recorded unattributed. */
  playerLabel?: string
}

export interface GameParticipantResult {
  side: number
  heroId: number
  heroName: string
  healthRemaining: number
  isWinner: boolean
}

export interface GameResult {
  gameId: number
  gameNumber: number
  mapId: number
  mapName: string
  participants: GameParticipantResult[]
}

export interface BanResult {
  heroId: number
  heroName: string
  banType: BanType
}

export interface MatchResultDto {
  matchId: number
  tournamentId: number
  round: number
  playedAt: string
  /** Absent when the match has no external link. */
  externalLink?: string
  participants: MatchParticipantResult[]
  /** Ordered by game number. */
  games: GameResult[]
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
