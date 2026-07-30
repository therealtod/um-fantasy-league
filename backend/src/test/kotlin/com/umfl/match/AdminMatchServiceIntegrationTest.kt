package com.umfl.match

import com.umfl.common.MatchRuleException
import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.TournamentRepository
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.jdbc.core.simple.JdbcClient
import java.time.Instant
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Exercises the admin match write path against "Winter of Champions", which
 * the seed deliberately leaves with zero recorded matches — "Summer of
 * Legends" is left alone because other tests assert its exact fixtures.
 */
class AdminMatchServiceIntegrationTest @Autowired constructor(
    private val adminMatchService: AdminMatchService,
    private val matchResultQuery: MatchResultQuery,
    private val tournamentRepository: TournamentRepository,
    private val jdbcClient: JdbcClient,
) : PostgresIntegrationTest() {

    private fun winterOfChampionsId() = requireNotNull(tournamentRepository.findByName("Winter of Champions")?.id)

    private fun id(table: String, name: String): Long =
        jdbcClient.sql("select id from $table where name = :name").param("name", name).query(Long::class.java).single()

    private fun aliceVsRobinHood(mapId: Long = id("game_map", "Baskerville Manor")) = Triple(
        mapId,
        listOf(
            MatchParticipantInput("Tomas Ferreira", id("heroes", "Alice"), 6, true),
            MatchParticipantInput("Rina Okafor", id("heroes", "Robin Hood"), 2, false),
        ),
        emptyList<MatchBanInput>(),
    )

    @Test
    fun `records a match result against a tournament with none yet`() {
        val tournamentId = winterOfChampionsId()
        val (mapId, participants, bans) = aliceVsRobinHood()

        val recorded = adminMatchService.record(tournamentId, round = 1, mapId = mapId, playedAt = Instant.now(), participants = participants, bans = bans)

        assertEquals(1, matchResultQuery.findByTournament(tournamentId).size)
        assertEquals(2, recorded.participants.size)
        assertTrue(recorded.participants.single { it.heroName == "Alice" }.isWinner)
    }

    @Test
    fun `findByTournament narrows to one round when asked`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")
        val (_, participants, bans) = aliceVsRobinHood(mapId)
        adminMatchService.record(tournamentId, round = 1, mapId = mapId, playedAt = Instant.now(), participants = participants, bans = bans)
        adminMatchService.record(tournamentId, round = 2, mapId = mapId, playedAt = Instant.now(), participants = participants, bans = bans)

        assertEquals(2, matchResultQuery.findByTournament(tournamentId).size)
        val roundOneOnly = matchResultQuery.findByTournament(tournamentId, round = 1)
        assertEquals(listOf(1), roundOneOnly.map { it.round })
    }

    @Test
    fun `a map outside the tournament's board pool is rejected`() {
        val tournamentId = winterOfChampionsId()
        // Raptor Paddock is not in Winter of Champions' seeded map pool.
        val (_, participants, bans) = aliceVsRobinHood()
        val raptorPaddock = id("game_map", "Raptor Paddock")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(tournamentId, round = 1, mapId = raptorPaddock, playedAt = Instant.now(), participants = participants, bans = bans)
        }
        assertEquals(listOf(MatchRule.MAP_NOT_IN_POOL), error.violations.map { it.rule })
    }

    @Test
    fun `the same hero on both sides is rejected`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")
        val alice = id("heroes", "Alice")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(
                tournamentId,
                round = 1,
                mapId = mapId,
                playedAt = Instant.now(),
                participants = listOf(
                    MatchParticipantInput("Tomas Ferreira", alice, 6, true),
                    MatchParticipantInput("Rina Okafor", alice, 2, false),
                ),
                bans = emptyList(),
            )
        }
        assertEquals(listOf(MatchRule.DUPLICATE_HERO), error.violations.map { it.rule })
    }

    @Test
    fun `two winners on one match is rejected`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(
                tournamentId,
                round = 1,
                mapId = mapId,
                playedAt = Instant.now(),
                participants = listOf(
                    MatchParticipantInput("Tomas Ferreira", id("heroes", "Alice"), 6, true),
                    MatchParticipantInput("Rina Okafor", id("heroes", "Robin Hood"), 6, true),
                ),
                bans = emptyList(),
            )
        }
        assertEquals(listOf(MatchRule.MULTIPLE_WINNERS), error.violations.map { it.rule })
    }

    @Test
    fun `a nonexistent heroId is rejected with a 422, not a raw constraint violation`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(
                tournamentId,
                round = 1,
                mapId = mapId,
                playedAt = Instant.now(),
                participants = listOf(
                    MatchParticipantInput("Tomas Ferreira", 999_999L, 6, true),
                    MatchParticipantInput("Rina Okafor", id("heroes", "Robin Hood"), 2, false),
                ),
                bans = emptyList(),
            )
        }
        assertEquals(listOf(MatchRule.UNKNOWN_HERO), error.violations.map { it.rule })
    }

    /**
     * The point of dropping the `player` table: an admin can name a competitor
     * who exists nowhere in the database, or name nobody at all, and the result
     * still records and still scores. A blank label normalises to null so
     * "absent" has one representation, not two.
     */
    @Test
    fun `a player label is free text - an unknown name, a blank and none at all all record`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")

        val recorded = adminMatchService.record(
            tournamentId,
            round = 1,
            mapId = mapId,
            playedAt = Instant.now(),
            participants = listOf(
                MatchParticipantInput("A. N. Other", id("heroes", "Alice"), 6, true),
                MatchParticipantInput(null, id("heroes", "Robin Hood"), 2, false),
            ),
            bans = emptyList(),
        )

        assertEquals("A. N. Other", recorded.participants.single { it.heroName == "Alice" }.playerLabel)
        assertNull(recorded.participants.single { it.heroName == "Robin Hood" }.playerLabel)

        val blanked = adminMatchService.correct(
            tournamentId,
            recorded.matchId,
            round = 1,
            mapId = mapId,
            playedAt = recorded.playedAt,
            participants = listOf(
                MatchParticipantInput("   ", id("heroes", "Alice"), 6, true),
                MatchParticipantInput("Rina Okafor", id("heroes", "Robin Hood"), 2, false),
            ),
            bans = emptyList(),
        )

        assertNull(
            blanked.participants.single { it.heroName == "Alice" }.playerLabel,
            "a whitespace-only label is stored as null, not as an empty string",
        )
        assertEquals("Rina Okafor", blanked.participants.single { it.heroName == "Robin Hood" }.playerLabel)
    }

    @Test
    fun `a timed draw records with no winner`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")

        val recorded = adminMatchService.record(
            tournamentId,
            round = 1,
            mapId = mapId,
            playedAt = Instant.now(),
            participants = listOf(
                MatchParticipantInput("Tomas Ferreira", id("heroes", "Alice"), 7, false),
                MatchParticipantInput("Rina Okafor", id("heroes", "Robin Hood"), 5, false),
            ),
            bans = emptyList(),
        )

        assertTrue(!recorded.hasWinner)
    }

    @Test
    fun `correcting a match fully replaces its participants`() {
        val tournamentId = winterOfChampionsId()
        val (mapId, participants, bans) = aliceVsRobinHood()
        val recorded = adminMatchService.record(tournamentId, round = 1, mapId = mapId, playedAt = Instant.now(), participants = participants, bans = bans)

        val corrected = adminMatchService.correct(
            tournamentId,
            recorded.matchId,
            round = 1,
            mapId = mapId,
            playedAt = recorded.playedAt,
            participants = listOf(
                MatchParticipantInput("Dmitri Kovac", id("heroes", "King Arthur"), 8, true),
                MatchParticipantInput("Hana Sato", id("heroes", "Medusa"), 1, false),
            ),
            bans = emptyList(),
        )

        assertEquals(setOf("King Arthur", "Medusa"), corrected.participants.map { it.heroName }.toSet())
        assertEquals(recorded.matchId, corrected.matchId, "correcting reuses the same match id")
    }

    @Test
    fun `deleting a match removes its participants and bans too`() {
        val tournamentId = winterOfChampionsId()
        val (mapId, participants, _) = aliceVsRobinHood()
        val bans = listOf(MatchBanInput(id("heroes", "Bigfoot")))
        val recorded = adminMatchService.record(tournamentId, round = 1, mapId = mapId, playedAt = Instant.now(), participants = participants, bans = bans)

        adminMatchService.delete(tournamentId, recorded.matchId)

        assertNull(matchResultQuery.findById(recorded.matchId))
        val orphanedParticipants = jdbcClient.sql("select count(*) from match_participant where match_id = :id")
            .param("id", recorded.matchId).query(Int::class.java).single()
        val orphanedBans = jdbcClient.sql("select count(*) from match_ban where match_id = :id")
            .param("id", recorded.matchId).query(Int::class.java).single()
        assertEquals(0, orphanedParticipants)
        assertEquals(0, orphanedBans)
    }
}
