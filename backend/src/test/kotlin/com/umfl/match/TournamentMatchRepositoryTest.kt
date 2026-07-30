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
 * Proves the `TournamentMatch` aggregate's `Set`-mapped children round-trip
 * before anything in the admin match surface depends on it — the same
 * insurance V1 already has for `EntrySlot`'s `List`+`keyColumn` mapping, just
 * for the shape `match_participant`/`match_ban` actually need.
 */
class TournamentMatchRepositoryTest @Autowired constructor(
    private val tournamentMatchRepository: TournamentMatchRepository,
    private val tournamentRepository: TournamentRepository,
    private val jdbcClient: JdbcClient,
) : PostgresIntegrationTest() {

    private fun id(table: String, name: String): Long =
        jdbcClient.sql("select id from $table where name = :name").param("name", name).query(Long::class.java).single()

    @Test
    fun `a match with two participants and a ban round-trips`() {
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
                mapId = baskerville,
                playedAt = playedAt,
                participants = setOf(
                    MatchParticipant(playerLabel = "Tomas Ferreira", heroId = alice, healthRemaining = 6, isWinner = true),
                    MatchParticipant(playerLabel = "Rina Okafor", heroId = robinHood, healthRemaining = 2, isWinner = false),
                ),
                bans = setOf(MatchBan(heroId = bigfoot)),
            )
        )

        val reloaded = assertNotNull(tournamentMatchRepository.findById(requireNotNull(saved.id)).orElse(null))

        assertEquals(tournamentId, reloaded.tournamentId)
        assertEquals(1, reloaded.round)
        assertEquals(baskerville, reloaded.mapId)
        assertEquals(playedAt, reloaded.playedAt)
        assertEquals(2, reloaded.participants.size)
        assertEquals(1, reloaded.bans.size)
        assertTrue(reloaded.participants.any { it.heroId == alice && it.isWinner && it.healthRemaining == 6 })
        assertTrue(reloaded.participants.any { it.heroId == robinHood && !it.isWinner && it.healthRemaining == 2 })
        assertEquals(bigfoot, reloaded.bans.single().heroId)
    }

    @Test
    fun `saving again fully replaces the previous participants and bans`() {
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
                mapId = baskerville,
                playedAt = Instant.parse("2026-08-15T12:00:00Z"),
                participants = setOf(
                    MatchParticipant(playerLabel = "Tomas Ferreira", heroId = alice, healthRemaining = 6, isWinner = true),
                    MatchParticipant(playerLabel = "Rina Okafor", heroId = robinHood, healthRemaining = 2, isWinner = false),
                ),
                bans = emptySet(),
            )
        )

        val corrected = tournamentMatchRepository.save(
            saved.copy(
                participants = setOf(
                    MatchParticipant(playerLabel = "Tomas Ferreira", heroId = sherlock, healthRemaining = 4, isWinner = false),
                    MatchParticipant(playerLabel = "Rina Okafor", heroId = dracula, healthRemaining = 7, isWinner = true),
                ),
            )
        )

        val reloaded = assertNotNull(tournamentMatchRepository.findById(requireNotNull(corrected.id)).orElse(null))
        assertEquals(setOf(sherlock, dracula), reloaded.participants.map { it.heroId }.toSet())
        assertTrue(reloaded.participants.none { it.heroId == alice || it.heroId == robinHood })
    }
}
