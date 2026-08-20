package com.umfl.api

import com.umfl.hero.Hero
import com.umfl.map.GameMap
import com.umfl.match.BanResult
import com.umfl.match.BanType
import com.umfl.match.GameResult
import com.umfl.match.MatchParticipantResult
import com.umfl.match.MatchResult
import com.umfl.matchimport.MatchImportPreview
import com.umfl.matchimport.UnresolvedKind
import com.umfl.matchimport.UnresolvedReason
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
    /**
     * Every hero this side drafted for the series, fielded or not. The side is
     * this entry's position in `participants`. Must include every hero the
     * side then played, or the submission is `PLAYED_HERO_NOT_DRAFTED`.
     */
    val draftedHeroIds: List<Long> = emptyList(),
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
    /**
     * Whose draft this hero was struck out of, 0 or 1. Omit it for a `PRE_BAN`
     * -- that happens before sides are assigned -- and the server answers 422
     * `BAN_SIDE_INVALID` if one is sent anyway. Omitting it on a typed ban is
     * allowed but loses the attribution.
     */
    val side: Int? = null,
)

data class RecordMatchRequest(
    @field:NotNull(message = "round is required")
    @field:Positive(message = "round must be positive")
    val round: Int?,
    @field:NotNull(message = "playedAt is required")
    val playedAt: Instant?,
    /**
     * Required and unique within the tournament: it is the duplicate check that
     * stops the same match being imported twice. A match with no page anywhere
     * still needs an identifier of its own — see `V9__external_link_required.sql`.
     */
    @field:NotBlank(message = "externalLink is required")
    val externalLink: String?,
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
    val externalLink: String,
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

// ---------------------------------------------------------------------------
// Match import
// ---------------------------------------------------------------------------

data class ImportMatchRequest(
    @field:NotBlank(message = "sourceUrl is required")
    val sourceUrl: String?,
)

/**
 * A scraped match resolved against a tournament, for the admin to review.
 *
 * Not a recorded match and not a [RecordMatchRequest]: `round` is missing
 * because the source names its rounds rather than numbering them, and any id
 * the importer could not resolve is null with an entry in [unresolved]. The
 * client fills the gaps and submits the result through the normal record
 * endpoint, so this never bypasses `MatchResultPolicy`.
 */
data class MatchImportPreviewDto(
    val sourceUrl: String,
    val roundName: String?,
    val seriesFormat: String?,
    val playedAt: Instant?,
    val playedAtRaw: String?,
    val alreadyImportedMatchId: Long?,
    val participants: List<ImportedParticipantDto>,
    val games: List<ImportedGameDto>,
    val bans: List<ImportedBanDto>,
    val unresolved: List<UnresolvedNameDto>,
) {
    companion object {
        fun from(preview: MatchImportPreview) = MatchImportPreviewDto(
            sourceUrl = preview.sourceUrl,
            roundName = preview.roundName,
            seriesFormat = preview.seriesFormat,
            playedAt = preview.playedAt,
            playedAtRaw = preview.playedAtRaw,
            alreadyImportedMatchId = preview.alreadyImportedMatchId,
            participants = preview.participants.map {
                ImportedParticipantDto(it.playerLabel, it.draftedHeroIds)
            },
            games = preview.games.map { game ->
                ImportedGameDto(
                    gameNumber = game.gameNumber,
                    mapId = game.mapId,
                    mapName = game.mapName,
                    participants = game.participants.map {
                        ImportedGameParticipantDto(it.heroId, it.heroName, it.healthRemaining, it.isWinner)
                    },
                )
            },
            bans = preview.bans.map { ImportedBanDto(it.heroId, it.heroName, it.banType, it.side) },
            unresolved = preview.unresolved.map {
                UnresolvedNameDto(it.kind, it.sourceName, it.reason, it.mapId, it.message)
            },
        )
    }
}

data class ImportedParticipantDto(
    val playerLabel: String?,
    /** The full draft, unfielded picks included — the shape `RecordMatchRequest` wants. */
    val draftedHeroIds: List<Long>,
)

data class ImportedGameDto(
    val gameNumber: Int,
    val mapId: Long?,
    val mapName: String?,
    val participants: List<ImportedGameParticipantDto>,
)

data class ImportedGameParticipantDto(
    val heroId: Long?,
    val heroName: String?,
    val healthRemaining: Int,
    val isWinner: Boolean,
)

data class ImportedBanDto(
    val heroId: Long?,
    val heroName: String?,
    val banType: BanType,
    /** Whose draft it came out of; null for a `PRE_BAN`. */
    val side: Int?,
)

data class UnresolvedNameDto(
    val kind: UnresolvedKind,
    val sourceName: String,
    val reason: UnresolvedReason,
    val mapId: Long?,
    val message: String,
)
