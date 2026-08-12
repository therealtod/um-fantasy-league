package com.umfl.match

import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.TournamentRepository
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.jdbc.core.simple.JdbcClient
import java.time.Instant
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * Proves the `TournamentMatch` aggregate's nested children round-trip before
 * anything in the admin match surface depends on it — the same insurance V1
 * already has for `EntrySlot`'s `List`+`keyColumn` mapping, just two levels
 * deep here (`TournamentMatch` → `games` → each game's own `participants`),
 * which is untested territory for Spring Data JDBC in this codebase.
 */
class TournamentMatchRepositoryTest @Autowired constructor(
    private val tournamentMatchRepository: TournamentMatchRepository,
    private val tournamentRepository: TournamentRepository,
    private val jdbcClient: JdbcClient,
) : PostgresIntegrationTest() {

    private fun id(table: String, name: String): Long =
        jdbcClient.sql("select id from $table where name = :name").param("name", name).query(Long::class.java).single()

    @Test
    fun `a match with two participants, one game and a ban round-trips`() {
        val tournamentId = requireNotNull(tournamentRepository.findByName("Winter of Champions")?.id)
        val alice = id("heroes", "Alice")
        val robinHood = id("heroes", "Robin Hood")
        val bigfoot = id("heroes", "Bigfoot")
        val baskerville = id("game_map", "Baskerville Manor")
        val playedAt = Instant.parse("2026-08-15T12:00:00Z")

        val saved = tournamentMatchRepository.save(
            TournamentMatch(
                tournamentId = tournamentId,
                round = 1,
                playedAt = playedAt,
                externalLink = "https://example.com/bracket/42",
                participants = listOf(
                    MatchParticipant(playerLabel = "Tomas Ferreira"),
                    MatchParticipant(playerLabel = "Rina Okafor"),
                ),
                games = setOf(
                    MatchGame(
                        tournamentId = tournamentId,
                        gameNumber = 1,
                        mapId = baskerville,
                        participants = listOf(
                            MatchGameParticipant(heroId = alice, healthRemaining = 6, isWinner = true),
                            MatchGameParticipant(heroId = robinHood, healthRemaining = 2, isWinner = false),
                        ),
                    ),
                ),
                bans = setOf(HeroBan(heroId = bigfoot, banType = BanType.PRE_BAN)),
            )
        )

        val reloaded = assertNotNull(tournamentMatchRepository.findById(requireNotNull(saved.id)).orElse(null))

        assertEquals(tournamentId, reloaded.tournamentId)
        assertEquals(1, reloaded.round)
        assertEquals(playedAt, reloaded.playedAt)
        assertEquals("https://example.com/bracket/42", reloaded.externalLink)
        assertEquals(listOf("Tomas Ferreira", "Rina Okafor"), reloaded.participants.map { it.playerLabel })
        assertEquals(1, reloaded.games.size)
        assertEquals(1, reloaded.bans.size)

        val game = reloaded.games.single()
        assertEquals(1, game.gameNumber)
        assertEquals(baskerville, game.mapId)
        assertEquals(2, game.participants.size)
        assertTrue(game.participants.any { it.heroId == alice && it.isWinner && it.healthRemaining == 6 })
        assertTrue(game.participants.any { it.heroId == robinHood && !it.isWinner && it.healthRemaining == 2 })

        val ban = reloaded.bans.single()
        assertEquals(bigfoot, ban.heroId)
        assertEquals(BanType.PRE_BAN, ban.banType)
    }

    @Test
    fun `a best-of-three series round-trips every game's own participants without misordering or collapsing`() {
        val tournamentId = requireNotNull(tournamentRepository.findByName("Winter of Champions")?.id)
        val medusa = id("heroes", "Medusa")
        val achilles = id("heroes", "Achilles")
        val baskerville = id("game_map", "Baskerville Manor")
        val sherwood = id("game_map", "Sherwood Forest")

        val saved = tournamentMatchRepository.save(
            TournamentMatch(
                tournamentId = tournamentId,
                round = 2,
                playedAt = Instant.parse("2026-08-16T09:00:00Z"),
                participants = listOf(
                    MatchParticipant(playerLabel = "Rina Okafor"),
                    MatchParticipant(playerLabel = "Dmitri Kovac"),
                ),
                games = setOf(
                    MatchGame(
                        tournamentId = tournamentId,
                        gameNumber = 1,
                        mapId = sherwood,
                        participants = listOf(
                            MatchGameParticipant(heroId = medusa, healthRemaining = 6, isWinner = true),
                            MatchGameParticipant(heroId = achilles, healthRemaining = 0, isWinner = false),
                        ),
                    ),
                    MatchGame(
                        tournamentId = tournamentId,
                        gameNumber = 2,
                        mapId = baskerville,
                        participants = listOf(
                            MatchGameParticipant(heroId = medusa, healthRemaining = 0, isWinner = false),
                            MatchGameParticipant(heroId = achilles, healthRemaining = 5, isWinner = true),
                        ),
                    ),
                    MatchGame(
                        tournamentId = tournamentId,
                        gameNumber = 3,
                        mapId = sherwood,
                        participants = listOf(
                            MatchGameParticipant(heroId = medusa, healthRemaining = 3, isWinner = true),
                            MatchGameParticipant(heroId = achilles, healthRemaining = 0, isWinner = false),
                        ),
                    ),
                ),
            )
        )

        val reloaded = assertNotNull(tournamentMatchRepository.findById(requireNotNull(saved.id)).orElse(null))

        assertEquals(3, reloaded.games.size)
        val gamesByNumber = reloaded.games.associateBy { it.gameNumber }
        assertEquals(setOf(1, 2, 3), gamesByNumber.keys)

        val game1 = gamesByNumber.getValue(1)
        assertEquals(sherwood, game1.mapId)
        assertEquals(2, game1.participants.size)
        assertTrue(game1.participants.any { it.heroId == medusa && it.isWinner && it.healthRemaining == 6 })
        assertTrue(game1.participants.any { it.heroId == achilles && !it.isWinner && it.healthRemaining == 0 })

        val game2 = gamesByNumber.getValue(2)
        assertEquals(baskerville, game2.mapId)
        assertTrue(game2.participants.any { it.heroId == achilles && it.isWinner && it.healthRemaining == 5 })
        assertTrue(game2.participants.any { it.heroId == medusa && !it.isWinner && it.healthRemaining == 0 })

        val game3 = gamesByNumber.getValue(3)
        assertEquals(sherwood, game3.mapId)
        assertTrue(game3.participants.any { it.heroId == medusa && it.isWinner && it.healthRemaining == 3 })
    }

    @Test
    fun `saving again fully replaces the previous participants, games and bans`() {
        val tournamentId = requireNotNull(tournamentRepository.findByName("Winter of Champions")?.id)
        val alice = id("heroes", "Alice")
        val robinHood = id("heroes", "Robin Hood")
        val sherlock = id("heroes", "Sherlock Holmes")
        val dracula = id("heroes", "Dracula")
        val baskerville = id("game_map", "Baskerville Manor")

        val saved = tournamentMatchRepository.save(
            TournamentMatch(
                tournamentId = tournamentId,
                round = 1,
                playedAt = Instant.parse("2026-08-15T12:00:00Z"),
                participants = listOf(
                    MatchParticipant(playerLabel = "Tomas Ferreira"),
                    MatchParticipant(playerLabel = "Rina Okafor"),
                ),
                games = setOf(
                    MatchGame(
                        tournamentId = tournamentId,
                        gameNumber = 1,
                        mapId = baskerville,
                        participants = listOf(
                            MatchGameParticipant(heroId = alice, healthRemaining = 6, isWinner = true),
                            MatchGameParticipant(heroId = robinHood, healthRemaining = 0, isWinner = false),
                        ),
                    ),
                ),
                bans = emptySet(),
            )
        )

        val corrected = tournamentMatchRepository.save(
            saved.copy(
                games = setOf(
                    MatchGame(
                        tournamentId = tournamentId,
                        gameNumber = 1,
                        mapId = baskerville,
                        participants = listOf(
                            MatchGameParticipant(heroId = sherlock, healthRemaining = 0, isWinner = false),
                            MatchGameParticipant(heroId = dracula, healthRemaining = 7, isWinner = true),
                        ),
                    ),
                ),
            )
        )

        val reloaded = assertNotNull(tournamentMatchRepository.findById(requireNotNull(corrected.id)).orElse(null))
        val heroIds = reloaded.games.single().participants.map { it.heroId }.toSet()
        assertEquals(setOf(sherlock, dracula), heroIds)
        assertTrue(heroIds.none { it == alice || it == robinHood })
    }
}
