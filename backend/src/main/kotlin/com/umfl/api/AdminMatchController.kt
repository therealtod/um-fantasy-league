package com.umfl.api

import com.umfl.auth.CurrentManager
import com.umfl.manager.Manager
import com.umfl.match.AdminMatchService
import com.umfl.match.MatchBanInput
import com.umfl.match.MatchParticipantInput
import com.umfl.match.MatchResultQuery
import com.umfl.tournament.TournamentService
import jakarta.validation.Valid
import org.springframework.http.HttpStatus
import org.springframework.security.access.prepost.PreAuthorize
import org.springframework.web.bind.annotation.DeleteMapping
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.PostMapping
import org.springframework.web.bind.annotation.PutMapping
import org.springframework.web.bind.annotation.RequestBody
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.RequestParam
import org.springframework.web.bind.annotation.ResponseStatus
import org.springframework.web.bind.annotation.RestController

/**
 * Admin-only: record, correct or retract a match result. Guarded twice —
 * `hasRole("ADMIN")` URL matchers in [com.umfl.config.SecurityConfig] /
 * the `!prod` chain in the same file, and the `@PreAuthorize` below — every
 * method here already runs as an admin by the time it's reached, so
 * `@CurrentManager` is only for knowing which admin acted.
 */
@RestController
@RequestMapping("/api/admin/tournaments/{tournamentId}/matches")
@PreAuthorize("hasRole('ADMIN')")
class AdminMatchController(
    private val adminMatchService: AdminMatchService,
    private val matchResultQuery: MatchResultQuery,
    private val tournamentService: TournamentService,
) {

    @GetMapping
    fun listMatches(
        @PathVariable tournamentId: Long,
        @RequestParam(required = false) round: Int?,
        @CurrentManager admin: Manager,
    ): List<MatchResultDto> {
        tournamentService.requireTournament(tournamentId)
        return matchResultQuery
            .findByTournament(tournamentId, round)
            .sortedByDescending { it.playedAt }
            .map(MatchResultDto::from)
    }

    @GetMapping("/{matchId}")
    fun getMatch(
        @PathVariable tournamentId: Long,
        @PathVariable matchId: Long,
        @CurrentManager admin: Manager,
    ): MatchResultDto {
        tournamentService.requireTournament(tournamentId)
        return matchResultQuery
            .findById(matchId)
            ?.takeIf { it.tournamentId == tournamentId }
            ?.let(MatchResultDto::from)
            ?: throw com.umfl.common.NotFoundException("No match $matchId in tournament $tournamentId")
    }

    @PostMapping
    @ResponseStatus(HttpStatus.CREATED)
    fun record(
        @PathVariable tournamentId: Long,
        @Valid @RequestBody request: RecordMatchRequest,
        @CurrentManager admin: Manager,
    ): MatchResultDto = MatchResultDto.from(
        adminMatchService.record(
            tournamentId = tournamentId,
            round = requireNotNull(request.round),
            mapId = requireNotNull(request.mapId),
            playedAt = requireNotNull(request.playedAt),
            participants = request.participants.orEmpty().map { it.toInput() },
            bans = request.bans.map { it.toInput() },
        )
    )

    @PutMapping("/{matchId}")
    fun correct(
        @PathVariable tournamentId: Long,
        @PathVariable matchId: Long,
        @Valid @RequestBody request: RecordMatchRequest,
        @CurrentManager admin: Manager,
    ): MatchResultDto = MatchResultDto.from(
        adminMatchService.correct(
            tournamentId = tournamentId,
            matchId = matchId,
            round = requireNotNull(request.round),
            mapId = requireNotNull(request.mapId),
            playedAt = requireNotNull(request.playedAt),
            participants = request.participants.orEmpty().map { it.toInput() },
            bans = request.bans.map { it.toInput() },
        )
    )

    @DeleteMapping("/{matchId}")
    @ResponseStatus(HttpStatus.NO_CONTENT)
    fun delete(
        @PathVariable tournamentId: Long,
        @PathVariable matchId: Long,
        @CurrentManager admin: Manager,
    ) {
        adminMatchService.delete(tournamentId, matchId)
    }
}

private fun MatchParticipantRequest.toInput() = MatchParticipantInput(
    playerLabel = playerLabel,
    heroId = requireNotNull(heroId),
    healthRemaining = requireNotNull(healthRemaining),
    isWinner = isWinner,
)

private fun MatchBanRequest.toInput() = MatchBanInput(heroId = requireNotNull(heroId))
