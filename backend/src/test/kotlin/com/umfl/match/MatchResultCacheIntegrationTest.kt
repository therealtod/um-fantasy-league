package com.umfl.match

import com.umfl.standings.StandingsService
import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.TournamentRepository
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import java.time.Instant
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * The cache against the queries it stands in front of, on a real database.
 *
 * The unit tests next door cover the caching mechanics with a stubbed query.
 * What needs Postgres is the one claim those cannot check: that
 * [MatchResultCache.findByTournamentSince] — a reverse, a filter and a take over
 * a list Kotlin holds — returns exactly what
 * [MatchResultQuery.findByTournamentSince]'s `order by played_at desc, id desc`
 * returns. The seed's Summer tournament is the right fixture for it because it
 * has parallel tables that share a `played_at`, which is precisely the tie the
 * two orderings could disagree about.
 */
class MatchResultCacheIntegrationTest @Autowired constructor(
    private val matchResultCache: MatchResultCache,
    private val matchResultQuery: MatchResultQuery,
    private val adminMatchService: AdminMatchService,
    private val standingsService: StandingsService,
    private val tournamentRepository: TournamentRepository,
) : PostgresIntegrationTest() {

    private fun summerId(): Long =
        requireNotNull(assertNotNull(tournamentRepository.findByName("Summer of Legends")).id)

    @Test
    fun `the cached ticker slice is identical to the query it replaces`() {
        val summer = summerId()
        val allMatches = matchResultQuery.findByTournament(summer)
        assertTrue(allMatches.size > 1, "the fixture needs several matches to be worth comparing")

        // Deliberately includes ids on either side of the range and a limit
        // longer than the tournament, so truncation and exhaustion are both hit.
        for (sinceMatchId in listOf(0L, 1L, 6L, 10L, 13L, 99L)) {
            for (limit in listOf(1, 3, 13, 25, 200)) {
                assertEquals(
                    matchResultQuery.findByTournamentSince(summer, sinceMatchId, limit),
                    matchResultCache.findByTournamentSince(summer, sinceMatchId, limit),
                    "slice disagreed with the SQL at sinceMatchId=$sinceMatchId limit=$limit",
                )
            }
        }
    }

    @Test
    fun `a cached list is reused rather than re-queried`() {
        val summer = summerId()

        val first = matchResultCache.findByTournament(summer)
        val second = matchResultCache.findByTournament(summer)

        assertEquals(first, second)
        assertTrue(first.isNotEmpty(), "the seed should have recorded matches")
    }

    /**
     * The half of the invalidation pair that only an in-transaction caller can
     * exercise. Every test here rolls back, so the after-completion listener
     * cannot be what makes this pass — the immediate one has to be.
     */
    @Test
    fun `a match recorded in this transaction is visible to the very next read`() {
        val summer = summerId()
        val before = matchResultCache.findByTournament(summer)

        recordAMatch(summer)

        val after = matchResultCache.findByTournament(summer)
        assertEquals(before.size + 1, after.size, "the cache served a list from before the write")
    }

    /**
     * The same, one level up — through [StandingsService] rather than the cache
     * directly, so the wiring is covered too.
     *
     * Asserted on the ticker rather than the board because the ticker is a
     * function of the matches alone. Whether a new match moves a *board* depends
     * on some entry having rostered one of its heroes, which is a fact about the
     * fixture rather than about the cache; `StandingsIntegrationTest` covers
     * that case deliberately, with a hero it picks for the purpose.
     */
    @Test
    fun `a match recorded after a first read reaches the ticker`() {
        val summer = summerId()
        val before = standingsService.ticker(summer, sinceMatchId = 0, limit = 200)

        recordAMatch(summer)

        val after = standingsService.ticker(summer, sinceMatchId = 0, limit = 200)
        assertEquals(before.size + 1, after.size, "the ticker was served a list from before the write")
        assertTrue(
            after.any { it.externalLink == "urn:umfl:match:cache-test" },
            "the newly recorded match never appeared",
        )
    }

    /**
     * Replays whatever the seed's newest match already looks like rather than
     * hand-building a legal one — [MatchResultPolicy] has opinions about
     * winners, health and drafts, and the subject here is the cache, not the
     * rules. Both participant lists are sorted by `side`, since on the way in
     * the side *is* the list position.
     */
    private fun recordAMatch(tournamentId: Long) {
        val template = matchResultQuery.findByTournament(tournamentId).last()
        adminMatchService.record(
            tournamentId = tournamentId,
            round = template.round,
            playedAt = Instant.now(),
            externalLink = "urn:umfl:match:cache-test",
            participants = template.participants.sortedBy { it.side }.map { participant ->
                MatchParticipantInput(
                    playerLabel = participant.playerLabel,
                    draftedHeroIds = participant.draftedHeroes.map { it.heroId },
                )
            },
            games = template.games.map { game ->
                MatchGameInput(
                    gameNumber = game.gameNumber,
                    mapId = game.mapId,
                    participants = game.participants.sortedBy { it.side }.map { participant ->
                        MatchGameParticipantInput(
                            heroId = participant.heroId,
                            healthRemaining = participant.healthRemaining,
                            isWinner = participant.isWinner,
                        )
                    },
                )
            },
            bans = emptyList(),
        )
    }
}
