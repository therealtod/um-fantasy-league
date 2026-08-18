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

    /**
     * Both sides draft every hero these fixtures field. A recorded draft has to
     * cover whatever the games use (`PLAYED_HERO_NOT_DRAFTED`), and the games
     * below trade heroes across sides freely, so one shared draft keeps each
     * test about the rule it is actually testing. Bigfoot is deliberately not
     * in it: it is the hero these fixtures ban.
     */
    private fun fixtureDraft() =
        listOf("Alice", "Robin Hood", "Medusa", "Achilles", "King Arthur").map { id("heroes", it) }

    private fun participants(
        label1: String? = "Tomas Ferreira",
        label2: String? = "Rina Okafor",
        drafted: List<Long> = fixtureDraft(),
    ) = listOf(MatchParticipantInput(label1, drafted), MatchParticipantInput(label2, drafted))

    private fun oneGame(
        mapId: Long,
        heroA: Long,
        heroB: Long,
        healthA: Int = 6,
        winnerA: Boolean = true,
        healthB: Int = 0,
        winnerB: Boolean = false,
        gameNumber: Int = 1,
    ) = listOf(
        MatchGameInput(
            gameNumber = gameNumber,
            mapId = mapId,
            participants = listOf(
                MatchGameParticipantInput(heroId = heroA, healthRemaining = healthA, isWinner = winnerA),
                MatchGameParticipantInput(heroId = heroB, healthRemaining = healthB, isWinner = winnerB),
            ),
        )
    )

    private fun aliceVsRobinHood(mapId: Long = id("game_map", "Baskerville Manor")) = Triple(
        oneGame(mapId, heroA = id("heroes", "Alice"), heroB = id("heroes", "Robin Hood")),
        participants(),
        emptyList<MatchBanInput>(),
    )

    @Test
    fun `records a match result against a tournament with none yet`() {
        val tournamentId = winterOfChampionsId()
        val (games, sides, bans) = aliceVsRobinHood()

        val recorded = adminMatchService.record(
            tournamentId, round = 1, playedAt = Instant.now(), externalLink = null,
            participants = sides, games = games, bans = bans,
        )

        assertEquals(1, matchResultQuery.findByTournament(tournamentId).size)
        assertEquals(2, recorded.participants.size)
        assertEquals(1, recorded.games.size)
        assertTrue(recorded.games.single().participants.single { it.heroName == "Alice" }.isWinner)
    }

    @Test
    fun `findByTournament narrows to one round when asked`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")
        val (games, sides, bans) = aliceVsRobinHood(mapId)
        adminMatchService.record(tournamentId, round = 1, playedAt = Instant.now(), externalLink = null, participants = sides, games = games, bans = bans)
        adminMatchService.record(tournamentId, round = 2, playedAt = Instant.now(), externalLink = null, participants = sides, games = games, bans = bans)

        assertEquals(2, matchResultQuery.findByTournament(tournamentId).size)
        val roundOneOnly = matchResultQuery.findByTournament(tournamentId, round = 1)
        assertEquals(listOf(1), roundOneOnly.map { it.round })
    }

    @Test
    fun `findByTournamentNewestFirst reverses the fold's order and honours its limit`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")
        val (games, sides, bans) = aliceVsRobinHood(mapId)
        val playedAt = Instant.parse("2026-03-01T18:00:00Z")
        repeat(3) { round ->
            adminMatchService.record(
                tournamentId, round = round + 1, playedAt = playedAt.plusSeconds(3600L * round),
                externalLink = null, participants = sides, games = games, bans = bans,
            )
        }

        val newestFirst = matchResultQuery.findByTournamentNewestFirst(tournamentId, round = null, limit = 100)
        assertEquals(listOf(3, 2, 1), newestFirst.map { it.round })
        assertEquals(matchResultQuery.findByTournament(tournamentId).reversed(), newestFirst)

        // The bound takes the newest page, not the oldest one.
        assertEquals(listOf(3, 2), matchResultQuery.findByTournamentNewestFirst(tournamentId, round = null, limit = 2).map { it.round })
        assertEquals(listOf(2), matchResultQuery.findByTournamentNewestFirst(tournamentId, round = 2, limit = 100).map { it.round })
    }

    @Test
    fun `records a best-of-three series with a hero repeated across games and different maps`() {
        val tournamentId = winterOfChampionsId()
        val baskerville = id("game_map", "Baskerville Manor")
        val sherwood = id("game_map", "Sherwood Forest")
        val medusa = id("heroes", "Medusa")
        val achilles = id("heroes", "Achilles")

        val recorded = adminMatchService.record(
            tournamentId,
            round = 1,
            playedAt = Instant.now(),
            externalLink = "https://example.com/bracket/7",
            participants = participants("Rina Okafor", "Dmitri Kovac"),
            games = listOf(
                MatchGameInput(1, sherwood, listOf(MatchGameParticipantInput(medusa, 6, true), MatchGameParticipantInput(achilles, 0, false))),
                MatchGameInput(2, baskerville, listOf(MatchGameParticipantInput(medusa, 0, false), MatchGameParticipantInput(achilles, 5, true))),
                MatchGameInput(3, sherwood, listOf(MatchGameParticipantInput(medusa, 3, true), MatchGameParticipantInput(achilles, 0, false))),
            ),
            bans = emptyList(),
        )

        assertEquals(3, recorded.games.size)
        assertEquals(listOf(1, 2, 3), recorded.games.map { it.gameNumber })
        assertEquals(listOf(sherwood, baskerville, sherwood), recorded.games.map { it.mapId })
        assertTrue(recorded.games[0].participants.single { it.heroName == "Medusa" }.isWinner)
        assertTrue(recorded.games[1].participants.single { it.heroName == "Achilles" }.isWinner)
        assertTrue(recorded.games[2].participants.single { it.heroName == "Medusa" }.isWinner)
    }

    @Test
    fun `a map outside the tournament's board pool is rejected`() {
        val tournamentId = winterOfChampionsId()
        // Raptor Paddock is not in Winter of Champions' seeded map pool.
        val raptorPaddock = id("game_map", "Raptor Paddock")
        val (_, sides, bans) = aliceVsRobinHood()
        val games = oneGame(raptorPaddock, heroA = id("heroes", "Alice"), heroB = id("heroes", "Robin Hood"))

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(tournamentId, round = 1, playedAt = Instant.now(), externalLink = null, participants = sides, games = games, bans = bans)
        }
        assertEquals(listOf(MatchRule.MAP_NOT_IN_POOL), error.violations.map { it.rule })
    }

    @Test
    fun `the same hero on both sides of a game is rejected`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")
        val alice = id("heroes", "Alice")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(
                tournamentId,
                round = 1,
                playedAt = Instant.now(),
                externalLink = null,
                participants = participants(),
                games = oneGame(mapId, heroA = alice, heroB = alice),
                bans = emptyList(),
            )
        }
        assertEquals(listOf(MatchRule.DUPLICATE_HERO), error.violations.map { it.rule })
    }

    @Test
    fun `two winners on one game is rejected`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(
                tournamentId,
                round = 1,
                playedAt = Instant.now(),
                externalLink = null,
                participants = participants(),
                games = oneGame(mapId, heroA = id("heroes", "Alice"), heroB = id("heroes", "Robin Hood"), winnerB = true),
                bans = emptyList(),
            )
        }
        assertEquals(listOf(MatchRule.NOT_EXACTLY_ONE_WINNER), error.violations.map { it.rule })
    }

    @Test
    fun `a nonexistent heroId is rejected with a 422, not a raw constraint violation`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(
                tournamentId,
                round = 1,
                playedAt = Instant.now(),
                externalLink = null,
                participants = participants(drafted = fixtureDraft() + 999_999L),
                games = oneGame(mapId, heroA = 999_999L, heroB = id("heroes", "Robin Hood")),
                bans = emptyList(),
            )
        }
        assertEquals(listOf(MatchRule.UNKNOWN_HERO), error.violations.map { it.rule })
    }

    @Test
    fun `a hero banned then played in a later game is rejected`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")
        val alice = id("heroes", "Alice")
        val robinHood = id("heroes", "Robin Hood")
        val bigfoot = id("heroes", "Bigfoot")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(
                tournamentId,
                round = 1,
                playedAt = Instant.now(),
                externalLink = null,
                participants = participants(drafted = fixtureDraft() + bigfoot),
                games = listOf(
                    MatchGameInput(1, mapId, listOf(MatchGameParticipantInput(alice, 6, true), MatchGameParticipantInput(robinHood, 0, false))),
                    MatchGameInput(2, mapId, listOf(MatchGameParticipantInput(bigfoot, 4, true), MatchGameParticipantInput(robinHood, 0, false))),
                ),
                bans = listOf(MatchBanInput(bigfoot, BanType.PRE_BAN)),
            )
        }
        // A hero cannot be struck out of the draft and then taken in it, so a
        // banned hero that played breaks both ban rules rather than one.
        assertEquals(
            setOf(MatchRule.BANNED_HERO_DRAFTED, MatchRule.BANNED_HERO_PLAYED),
            error.violations.map { it.rule }.toSet(),
        )
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
            playedAt = Instant.now(),
            externalLink = null,
            participants = participants("A. N. Other", null),
            games = oneGame(mapId, heroA = id("heroes", "Alice"), heroB = id("heroes", "Robin Hood")),
            bans = emptyList(),
        )

        assertEquals("A. N. Other", recorded.playerLabelForSide(0))
        assertNull(recorded.playerLabelForSide(1))

        val blanked = adminMatchService.correct(
            tournamentId,
            recorded.matchId,
            round = 1,
            playedAt = recorded.playedAt,
            externalLink = null,
            participants = participants("   ", "Rina Okafor"),
            games = oneGame(mapId, heroA = id("heroes", "Alice"), heroB = id("heroes", "Robin Hood")),
            bans = emptyList(),
        )

        assertNull(blanked.playerLabelForSide(0), "a whitespace-only label is stored as null, not as an empty string")
        assertEquals("Rina Okafor", blanked.playerLabelForSide(1))
    }

    @Test
    fun `a game with no winner at all is rejected, not stored as a draw`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(
                tournamentId,
                round = 1,
                playedAt = Instant.now(),
                externalLink = null,
                participants = participants(),
                games = oneGame(mapId, heroA = id("heroes", "Alice"), heroB = id("heroes", "Robin Hood"), healthA = 0, winnerA = false, healthB = 0),
                bans = emptyList(),
            )
        }
        assertEquals(listOf(MatchRule.NOT_EXACTLY_ONE_WINNER), error.violations.map { it.rule })
    }

    @Test
    fun `a game with a positive-health loser is rejected`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(
                tournamentId,
                round = 1,
                playedAt = Instant.now(),
                externalLink = null,
                participants = participants(),
                games = oneGame(mapId, heroA = id("heroes", "Alice"), heroB = id("heroes", "Robin Hood"), healthA = 7, healthB = 5),
                bans = emptyList(),
            )
        }

        assertEquals(listOf(MatchRule.LOSER_HAS_POSITIVE_HEALTH), error.violations.map { it.rule })
    }

    @Test
    fun `a game with a negative-health loser is accepted`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")

        val recorded = adminMatchService.record(
            tournamentId,
            round = 1,
            playedAt = Instant.now(),
            externalLink = null,
            participants = participants(),
            games = oneGame(mapId, heroA = id("heroes", "Alice"), heroB = id("heroes", "Robin Hood"), healthA = 7, healthB = -3),
            bans = emptyList(),
        )

        val loser = recorded.games.single().participants.single { !it.isWinner }
        assertEquals(-3, loser.healthRemaining)
    }

    @Test
    fun `correcting a match fully replaces its participants and games, and persists an external link`() {
        val tournamentId = winterOfChampionsId()
        val (games, sides, bans) = aliceVsRobinHood()
        val recorded = adminMatchService.record(tournamentId, round = 1, playedAt = Instant.now(), externalLink = null, participants = sides, games = games, bans = bans)
        assertNull(recorded.externalLink)

        val corrected = adminMatchService.correct(
            tournamentId,
            recorded.matchId,
            round = 1,
            playedAt = recorded.playedAt,
            externalLink = "https://example.com/bracket/42",
            participants = participants("Dmitri Kovac", "Hana Sato"),
            games = oneGame(
                id("game_map", "Baskerville Manor"),
                heroA = id("heroes", "King Arthur"),
                heroB = id("heroes", "Medusa"),
                healthA = 8,
                healthB = 0,
            ),
            bans = emptyList(),
        )

        assertEquals(setOf("King Arthur", "Medusa"), corrected.games.single().participants.map { it.heroName }.toSet())
        assertEquals(recorded.matchId, corrected.matchId, "correcting reuses the same match id")
        assertEquals("https://example.com/bracket/42", corrected.externalLink)
    }

    @Test
    fun `a blank external link is stored as null, the same way a blank player label is`() {
        val tournamentId = winterOfChampionsId()
        val (games, sides, bans) = aliceVsRobinHood()

        // What the admin form posts for an untouched optional text input. Left as
        // "", it would come back out of the API as an external link that is there
        // for a null check and absent for a human.
        val recorded = adminMatchService.record(tournamentId, round = 1, playedAt = Instant.now(), externalLink = "  ", participants = sides, games = games, bans = bans)

        assertNull(recorded.externalLink)
    }

    @Test
    fun `deleting a match removes its participants, games, game participants and bans`() {
        val tournamentId = winterOfChampionsId()
        val (games, sides, _) = aliceVsRobinHood()
        val bans = listOf(MatchBanInput(id("heroes", "Bigfoot"), BanType.SELF_BAN))
        val recorded = adminMatchService.record(tournamentId, round = 1, playedAt = Instant.now(), externalLink = null, participants = sides, games = games, bans = bans)
        val gameId = recorded.games.single().gameId

        adminMatchService.delete(tournamentId, recorded.matchId)

        assertNull(matchResultQuery.findById(recorded.matchId))
        fun countWhere(table: String, column: String, value: Long): Int =
            jdbcClient.sql("select count(*) from $table where $column = :id").param("id", value).query(Int::class.java).single()

        assertEquals(0, countWhere("match_participant", "match_id", recorded.matchId))
        assertEquals(0, countWhere("match_game", "match_id", recorded.matchId))
        assertEquals(0, countWhere("match_game_participant", "game_id", gameId))
        assertEquals(0, countWhere("hero_ban", "match_id", recorded.matchId))
        assertEquals(0, countWhere("match_hero_pick", "match_id", recorded.matchId))
    }

    @Test
    fun `each side's draft round-trips, including a hero it never fielded`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")
        val alice = id("heroes", "Alice")
        val robinHood = id("heroes", "Robin Hood")
        val medusa = id("heroes", "Medusa")

        val recorded = adminMatchService.record(
            tournamentId,
            round = 1,
            playedAt = Instant.now(),
            externalLink = null,
            participants = listOf(
                MatchParticipantInput("Tomas Ferreira", listOf(alice, medusa)),
                MatchParticipantInput("Rina Okafor", listOf(robinHood)),
            ),
            games = oneGame(mapId, heroA = alice, heroB = robinHood),
            bans = emptyList(),
        )

        val sideZero = recorded.participants.single { it.side == 0 }
        assertEquals(listOf("Alice", "Medusa"), sideZero.draftedHeroes.map { it.heroName })
        assertEquals(listOf("Robin Hood"), recorded.participants.single { it.side == 1 }.draftedHeroes.map { it.heroName })
        assertTrue(
            recorded.games.single().participants.none { it.heroName == "Medusa" },
            "Medusa was drafted and never fielded -- exactly the case APPEARANCE now prices",
        )
    }

    @Test
    fun `a hero fielded by a side that never drafted it is rejected with a 422`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")
        val alice = id("heroes", "Alice")
        val robinHood = id("heroes", "Robin Hood")

        val error = assertFailsWith<MatchRuleException> {
            adminMatchService.record(
                tournamentId,
                round = 1,
                playedAt = Instant.now(),
                externalLink = null,
                participants = listOf(
                    MatchParticipantInput("Tomas Ferreira", listOf(alice)),
                    MatchParticipantInput("Rina Okafor", listOf(alice)),
                ),
                games = oneGame(mapId, heroA = alice, heroB = robinHood),
                bans = emptyList(),
            )
        }

        assertEquals(listOf(MatchRule.PLAYED_HERO_NOT_DRAFTED), error.violations.map { it.rule })
        assertTrue(error.violations.single().message.contains("$robinHood in game 1"))
    }

    @Test
    fun `correcting a match replaces the draft rather than adding to it`() {
        val tournamentId = winterOfChampionsId()
        val mapId = id("game_map", "Baskerville Manor")
        val alice = id("heroes", "Alice")
        val robinHood = id("heroes", "Robin Hood")
        val medusa = id("heroes", "Medusa")

        val recorded = adminMatchService.record(
            tournamentId,
            round = 1,
            playedAt = Instant.now(),
            externalLink = null,
            participants = listOf(
                MatchParticipantInput("Tomas Ferreira", listOf(alice, medusa)),
                MatchParticipantInput("Rina Okafor", listOf(robinHood)),
            ),
            games = oneGame(mapId, heroA = alice, heroB = robinHood),
            bans = emptyList(),
        )

        val corrected = adminMatchService.correct(
            tournamentId,
            recorded.matchId,
            round = 1,
            playedAt = recorded.playedAt,
            externalLink = null,
            participants = listOf(
                MatchParticipantInput("Tomas Ferreira", listOf(alice)),
                MatchParticipantInput("Rina Okafor", listOf(robinHood)),
            ),
            games = oneGame(mapId, heroA = alice, heroB = robinHood),
            bans = emptyList(),
        )

        assertEquals(
            listOf("Alice"),
            corrected.participants.single { it.side == 0 }.draftedHeroes.map { it.heroName },
            "the mistaken Medusa pick is gone, not merged",
        )
        assertEquals(
            2,
            jdbcClient.sql("select count(*) from match_hero_pick where match_id = :id")
                .param("id", recorded.matchId).query(Int::class.java).single(),
        )
    }
}
