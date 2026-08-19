package com.umfl.api

import com.umfl.auth.CurrentManager
import com.umfl.common.NotFoundException
import com.umfl.manager.Manager
import com.umfl.match.AdminMatchService
import com.umfl.match.MatchBanInput
import com.umfl.match.MatchGameInput
import com.umfl.match.MatchGameParticipantInput
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

    /**
     * Recorded matches, newest first, optionally narrowed to one [round].
     *
     * Bounded: every match comes back with all of its games, game participants
     * and bans attached, so an unbounded list is the one admin read whose cost
     * grows with the tournament forever. [limit] defaults to 200 — a page well
     * past any real tournament's match count — and is clamped the same way the
     * ticker's is in [StandingsController.matches].
     */
    @GetMapping
    fun listMatches(
        @PathVariable tournamentId: Long,
        @RequestParam(required = false) round: Int?,
        @RequestParam(required = false, defaultValue = "200") limit: Int,
        @CurrentManager admin: Manager,
    ): List<MatchResultDto> {
        tournamentService.requireTournament(tournamentId)
        return matchResultQuery
            .findByTournamentNewestFirst(tournamentId, round, limit.coerceIn(1, 500))
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
            ?: throw NotFoundException("No match $matchId in tournament $tournamentId")
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
            playedAt = requireNotNull(request.playedAt),
            externalLink = request.externalLink,
            participants = request.participants.orEmpty().map { it.toInput() },
            games = request.games.orEmpty().map { it.toInput() },
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
            playedAt = requireNotNull(request.playedAt),
            externalLink = request.externalLink,
            participants = request.participants.orEmpty().map { it.toInput() },
            games = request.games.orEmpty().map { it.toInput() },
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
    draftedHeroIds = draftedHeroIds,
)

private fun MatchGameParticipantRequest.toInput() = MatchGameParticipantInput(
    heroId = requireNotNull(heroId),
    healthRemaining = requireNotNull(healthRemaining),
    isWinner = isWinner,
)

private fun MatchGameRequest.toInput() = MatchGameInput(
    gameNumber = requireNotNull(gameNumber),
    mapId = requireNotNull(mapId),
    participants = participants.orEmpty().map { it.toInput() },
)

private fun MatchBanRequest.toInput() = MatchBanInput(
    heroId = requireNotNull(heroId),
    banType = requireNotNull(banType),
    side = side,
)
