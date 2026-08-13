package com.umfl.api

import com.umfl.hero.Hero
import com.umfl.map.GameMap
import com.umfl.match.BanResult
import com.umfl.match.BanType
import com.umfl.match.GameResult
import com.umfl.match.MatchParticipantResult
import com.umfl.match.MatchResult
import com.umfl.scoring.ScoringRuleSet
import com.umfl.tournament.TournamentFormat
import com.umfl.tournament.TournamentStatus
import jakarta.validation.Valid
import jakarta.validation.constraints.NotBlank
import jakarta.validation.constraints.NotNull
import jakarta.validation.constraints.Positive
import jakarta.validation.constraints.Size
import java.math.BigDecimal
import java.time.Instant
import java.time.LocalDate

// ---------------------------------------------------------------------------
// Tournaments
// ---------------------------------------------------------------------------

data class CreateTournamentRequest(
    @field:NotBlank(message = "name is required")
    val name: String?,
    @field:NotNull(message = "format is required")
    val format: TournamentFormat?,
    @field:NotNull(message = "status is required")
    val status: TournamentStatus?,
    @field:NotNull(message = "startDate is required")
    val startDate: LocalDate?,
    val endDate: LocalDate? = null,
    @field:Positive(message = "capacity must be positive")
    val capacity: Int?,
    @field:Positive(message = "rosterSize must be positive")
    val rosterSize: Int?,
    @field:Positive(message = "creditGrant must be positive")
    val creditGrant: Int?,
)

/** Full replace — every field is resubmitted, including status. */
typealias UpdateTournamentRequest = CreateTournamentRequest

// ---------------------------------------------------------------------------
// Heroes
// ---------------------------------------------------------------------------

data class CreateHeroRequest(
    @field:NotBlank(message = "name is required")
    val name: String?,
    val imageUrl: String? = null,
)

typealias UpdateHeroRequest = CreateHeroRequest

data class HeroAdminDto(
    val id: Long,
    val name: String,
    val imageUrl: String?,
) {
    companion object {
        fun from(hero: Hero) = HeroAdminDto(id = requireNotNull(hero.id), name = hero.name, imageUrl = hero.imageUrl)
    }
}

data class SetHeroCostRequest(
    @field:Positive(message = "cost must be positive")
    val cost: Int?,
)

data class HeroPoolEntryRequest(
    @field:NotNull(message = "heroId is required")
    val heroId: Long?,
    @field:Positive(message = "cost must be positive")
    val cost: Int?,
)

data class AddHeroesToPoolRequest(
    @field:Valid
    @field:Size(min = 1, message = "at least one hero is required")
    val heroes: List<HeroPoolEntryRequest>?,
)

// ---------------------------------------------------------------------------
// Maps
// ---------------------------------------------------------------------------

data class CreateMapRequest(
    @field:NotBlank(message = "name is required")
    val name: String?,
)

typealias UpdateMapRequest = CreateMapRequest

data class MapAdminDto(
    val id: Long,
    val name: String,
) {
    companion object {
        fun from(map: GameMap) = MapAdminDto(id = requireNotNull(map.id), name = map.name)
    }
}

data class AddMapsToPoolRequest(
    @field:Size(min = 1, message = "at least one map is required")
    val mapIds: List<Long>?,
)

// ---------------------------------------------------------------------------
// Match results
// ---------------------------------------------------------------------------

data class MatchParticipantRequest(
    /** Who piloted this side for the whole series. Free text, optional — there is no `player` table to validate against. */
    val playerLabel: String? = null,
)

data class MatchGameParticipantRequest(
    @field:NotNull(message = "heroId is required")
    val heroId: Long?,
    // No @PositiveOrZero here: a losing hero finishing on negative health is
    // legal (the loser-must-be-<=0 rule lives in MatchResultPolicy, which
    // needs to see negative values to enforce it), and the winner is
    // unrestricted by the same schema-level check constraint.
    @field:NotNull(message = "healthRemaining is required")
    val healthRemaining: Int?,
    val isWinner: Boolean = false,
)

data class MatchGameRequest(
    @field:NotNull(message = "gameNumber is required")
    @field:Positive(message = "gameNumber must be positive")
    val gameNumber: Int?,
    @field:NotNull(message = "mapId is required")
    val mapId: Long?,
    @field:Valid
    @field:Size(min = 2, max = 2, message = "exactly two sides are required")
    val participants: List<MatchGameParticipantRequest>?,
)

data class MatchBanRequest(
    @field:NotNull(message = "heroId is required")
    val heroId: Long?,
    @field:NotNull(message = "banType is required")
    val banType: BanType?,
)

data class RecordMatchRequest(
    @field:NotNull(message = "round is required")
    @field:Positive(message = "round must be positive")
    val round: Int?,
    @field:NotNull(message = "playedAt is required")
    val playedAt: Instant?,
    val externalLink: String? = null,
    @field:Valid
    @field:Size(min = 2, max = 2, message = "exactly two participants are required")
    val participants: List<MatchParticipantRequest>?,
    @field:Valid
    @field:Size(min = 1, message = "at least one game is required")
    val games: List<MatchGameRequest>?,
    @field:Valid
    val bans: List<MatchBanRequest> = emptyList(),
)

data class MatchResultDto(
    val matchId: Long,
    val tournamentId: Long,
    val round: Int,
    val playedAt: Instant,
    val externalLink: String?,
    val participants: List<MatchParticipantResult>,
    val games: List<GameResult>,
    val bans: List<BanResult>,
) {
    companion object {
        fun from(result: MatchResult) = MatchResultDto(
            matchId = result.matchId,
            tournamentId = result.tournamentId,
            round = result.round,
            playedAt = result.playedAt,
            externalLink = result.externalLink,
            participants = result.participants,
            games = result.games,
            bans = result.bans,
        )
    }
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

data class ScoringCoefficientRequest(
    @field:NotBlank(message = "metric is required")
    val metric: String?,
    @field:NotNull(message = "coefficient is required")
    val coefficient: BigDecimal?,
    val sortOrder: Int = 0,
)

data class CreateScoringRuleSetRequest(
    @field:NotBlank(message = "name is required")
    val name: String?,
    @field:Valid
    @field:Size(min = 1, message = "at least one coefficient is required")
    val coefficients: List<ScoringCoefficientRequest>?,
    /** Activate this rule set immediately, deactivating any current sibling. */
    val activate: Boolean = false,
)

data class UpdateScoringRuleSetRequest(
    @field:NotBlank(message = "name is required")
    val name: String?,
    @field:Valid
    @field:Size(min = 1, message = "at least one coefficient is required")
    val coefficients: List<ScoringCoefficientRequest>?,
)

data class ScoringRuleSetDto(
    val id: Long,
    val tournamentId: Long,
    val name: String,
    val isActive: Boolean,
    val coefficients: List<ScoringCoefficientDto>,
    /** Metrics no extractor implements — not rejected, just surfaced (e.g. the seed's CROWD_FAVOURITE). */
    val warnings: List<String>,
) {
    companion object {
        fun from(ruleSet: ScoringRuleSet, warnings: List<String> = emptyList()) = ScoringRuleSetDto(
            id = requireNotNull(ruleSet.id),
            tournamentId = ruleSet.tournamentId,
            name = ruleSet.name,
            isActive = ruleSet.isActive,
            coefficients = ruleSet.coefficients
                .sortedBy { it.sortOrder }
                .map { ScoringCoefficientDto(it.metric, it.coefficient, it.sortOrder) },
            warnings = warnings,
        )
    }
}

data class ScoringCoefficientDto(
    val metric: String,
    val coefficient: BigDecimal,
    val sortOrder: Int,
)
