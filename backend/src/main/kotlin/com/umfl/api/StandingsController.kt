package com.umfl.api

import com.umfl.standings.StandingsBoard
import com.umfl.standings.StandingsService
import com.umfl.standings.StandingsSseHub
import com.umfl.standings.TickerEntry
import com.umfl.tournament.TournamentService
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.RequestParam
import org.springframework.web.bind.annotation.RestController
import org.springframework.web.servlet.mvc.method.annotation.SseEmitter

@RestController
@RequestMapping("/api/tournaments/{id}")
class StandingsController(
    private val standingsService: StandingsService,
    private val tournamentService: TournamentService,
    private val standingsSseHub: StandingsSseHub,
) {

    /**
     * The leaderboard, carrying its own column definitions — the backend cannot
     * know which metrics exist until it has read `scoring_coefficient`.
     */
    @GetMapping("/standings")
    fun standings(@PathVariable id: Long): StandingsBoard {
        tournamentService.requireTournament(id)
        return standingsService.board(id)
    }

    /**
     * Recorded matches, newest first — the Standings ticker.
     *
     * Pass the highest `matchId` already seen as [sinceMatchId] to poll for just
     * the new results. The id is the key, not `playedAt`: parallel tables share a
     * timestamp, so `playedAt` is not unique.
     */
    @GetMapping("/matches")
    fun matches(
        @PathVariable id: Long,
        @RequestParam(required = false, defaultValue = "0") sinceMatchId: Long,
        @RequestParam(required = false, defaultValue = "25") limit: Int,
    ): List<TickerEntry> {
        tournamentService.requireTournament(id)
        return standingsService.ticker(id, sinceMatchId, limit.coerceIn(1, 200))
    }

    /**
     * A bare "something changed" push — no board/ticker payload duplicated over
     * the wire. The client already knows how to pull fresh data via
     * [standings]/[matches]; this just tells it when to.
     */
    @GetMapping("/standings/stream")
    fun standingsStream(@PathVariable id: Long): SseEmitter {
        tournamentService.requireTournament(id)
        return standingsSseHub.subscribe(id)
    }
}
