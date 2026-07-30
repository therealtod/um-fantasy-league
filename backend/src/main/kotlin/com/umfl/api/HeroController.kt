package com.umfl.api

import com.umfl.hero.HeroFilter
import com.umfl.hero.HeroQueryRepository
import com.umfl.hero.HeroSort
import com.umfl.tournament.TournamentService
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.RequestParam
import org.springframework.web.bind.annotation.RestController

/**
 * The hero pool that feeds the Roster Builder grid.
 *
 * It lives under the tournament because cost is tournament-scoped: a bare
 * `/api/heroes` could not answer the Roster Builder's actual question, which is
 * "what can I pick here, and what does it cost me".
 *
 * Deliberately thin — per-hero stat exploration is out of scope, and third-party
 * sites already publish Unmatched statistics.
 */
@RestController
@RequestMapping("/api/tournaments/{tournamentId}/heroes")
class HeroController(
    private val heroQueryRepository: HeroQueryRepository,
    private val tournamentService: TournamentService,
) {

    @GetMapping
    fun list(
        @PathVariable tournamentId: Long,
        @RequestParam(required = false) search: String?,
        @RequestParam(required = false, defaultValue = "COST") sort: HeroSort,
    ): List<HeroDto> {
        // 404 on an unknown tournament rather than a silently empty pool.
        tournamentService.requireTournament(tournamentId)
        return heroQueryRepository
            .findByTournament(tournamentId, HeroFilter(search = search, sort = sort))
            .map(HeroDto::from)
    }
}
