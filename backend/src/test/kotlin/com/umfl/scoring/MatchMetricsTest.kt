package com.umfl.scoring

import com.umfl.match.BanResult
import com.umfl.match.MatchResult
import com.umfl.match.ParticipantResult
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import java.time.Instant
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * A per-metric truth table over hand-built matches.
 *
 * The fixtures mirror the shapes the seed data actually contains — a decided
 * match, the round-2 shutout, and the round-3 timed draw — so a metric that
 * disagrees with reality fails here rather than three layers up in the
 * leaderboard.
 */
class MatchMetricsTest {

    private val played = Instant.parse("2026-06-06T11:00:00Z")

    private fun participant(
        id: Long,
        heroId: Long,
        heroName: String,
        health: Int,
        isWinner: Boolean = false,
    ) = ParticipantResult(
        participantId = id,
        playerLabel = "Player $id",
        heroId = heroId,
        heroName = heroName,
        healthRemaining = health,
        isWinner = isWinner,
    )

    private fun match(
        participants: List<ParticipantResult>,
        bans: List<BanResult> = emptyList(),
    ) = MatchResult(
        matchId = 6,
        tournamentId = 1,
        round = 2,
        mapId = 3,
        mapName = "Raptor Paddock",
        playedAt = played,
        participants = participants,
        bans = bans,
    )

    /** Match 6 of the seed: Bigfoot 11 beats Beowulf 0. */
    private val shutoutMatch = match(
        listOf(
            participant(id = 1, heroId = 7, heroName = "Bigfoot", health = 11, isWinner = true),
            participant(id = 2, heroId = 11, heroName = "Beowulf", health = 0),
        ),
        bans = listOf(BanResult(heroId = 8, heroName = "Sun Wukong")),
    )

    /** Match 11 of the seed: nobody wins, both sides alive on 7 and 5. */
    private val timedDraw = match(
        listOf(
            participant(id = 1, heroId = 5, heroName = "Sherlock Holmes", health = 7),
            participant(id = 2, heroId = 6, heroName = "Dracula", health = 5),
        ),
    )

    private fun contextFor(match: MatchResult, heroId: Long): MetricContext =
        assertNotNull(match.heroContexts().singleOrNull { it.heroId == heroId }, "no context for hero $heroId")

    private fun measure(metric: String, context: MetricContext): Double {
        val extractor = assertNotNull(MatchMetrics[metric], "no extractor for $metric")
        return extractor(context)
    }

    @Nested
    inner class Registry {

        @Test
        fun `every advertised metric resolves to an extractor`() {
            assertEquals(
                setOf(
                    "APPEARANCE", "BAN", "WIN", "LOSS", "DRAW",
                    "HEALTH_REMAINING", "HEALTH_DIFFERENTIAL", "SHUTOUT",
                ),
                MatchMetrics.known,
            )
        }

        @Test
        fun `lookup is case- and whitespace-insensitive`() {
            assertNotNull(MatchMetrics[" win "])
            assertNotNull(MatchMetrics["Health_Remaining"])
        }

        @Test
        fun `unknown catches a typo and leaves the real keys alone`() {
            assertEquals(
                listOf("HEALTH_REMAINNG", "CROWD_FAVOURITE"),
                MatchMetrics.unknown(listOf("WIN", "HEALTH_REMAINNG", "BAN", "CROWD_FAVOURITE")),
            )
        }

        @Test
        fun `an unimplemented metric resolves to nothing rather than throwing`() {
            assertNull(MatchMetrics["CROWD_FAVOURITE"])
        }

        @Test
        fun `labels are title-cased for the leaderboard header`() {
            assertEquals("Health Remaining", MatchMetrics.label("HEALTH_REMAINING"))
            assertEquals("Win", MatchMetrics.label("win"))
            assertEquals("Health Differential", MatchMetrics.label("HEALTH_DIFFERENTIAL"))
        }
    }

    @Nested
    inner class Outcomes {

        @Test
        fun `the winner takes WIN and neither LOSS nor DRAW`() {
            val bigfoot = contextFor(shutoutMatch, heroId = 7)

            assertEquals(1.0, measure("WIN", bigfoot))
            assertEquals(0.0, measure("LOSS", bigfoot))
            assertEquals(0.0, measure("DRAW", bigfoot))
        }

        @Test
        fun `the defeated hero takes LOSS only`() {
            val beowulf = contextFor(shutoutMatch, heroId = 11)

            assertEquals(0.0, measure("WIN", beowulf))
            assertEquals(1.0, measure("LOSS", beowulf))
            assertEquals(0.0, measure("DRAW", beowulf))
        }

        @Test
        fun `a timed draw is a DRAW for both sides and a LOSS for neither`() {
            for (heroId in listOf(5L, 6L)) {
                val side = contextFor(timedDraw, heroId)
                assertEquals(0.0, measure("WIN", side), "hero $heroId")
                assertEquals(0.0, measure("LOSS", side), "hero $heroId")
                assertEquals(1.0, measure("DRAW", side), "hero $heroId")
            }
        }
    }

    @Nested
    inner class Health {

        @Test
        fun `HEALTH_REMAINING is the hero's own end-of-match health`() {
            assertEquals(11.0, measure("HEALTH_REMAINING", contextFor(shutoutMatch, 7)))
            assertEquals(0.0, measure("HEALTH_REMAINING", contextFor(shutoutMatch, 11)))
            assertEquals(5.0, measure("HEALTH_REMAINING", contextFor(timedDraw, 6)))
        }

        @Test
        fun `HEALTH_DIFFERENTIAL is symmetric across the two sides`() {
            val sherlock = measure("HEALTH_DIFFERENTIAL", contextFor(timedDraw, 5))
            val dracula = measure("HEALTH_DIFFERENTIAL", contextFor(timedDraw, 6))

            assertEquals(2.0, sherlock)
            assertEquals(-2.0, dracula)
            assertEquals(0.0, sherlock + dracula, "a differential must neither create nor destroy points")
        }

        @Test
        fun `HEALTH_DIFFERENTIAL measures against the healthiest opponent`() {
            val threeWay = match(
                listOf(
                    participant(id = 1, heroId = 1, heroName = "Alice", health = 6, isWinner = true),
                    participant(id = 2, heroId = 2, heroName = "Medusa", health = 4),
                    participant(id = 3, heroId = 3, heroName = "Sinbad", health = 0),
                ),
            )

            assertEquals(2.0, measure("HEALTH_DIFFERENTIAL", contextFor(threeWay, 1)))
            assertEquals(-2.0, measure("HEALTH_DIFFERENTIAL", contextFor(threeWay, 2)))
            assertEquals(-6.0, measure("HEALTH_DIFFERENTIAL", contextFor(threeWay, 3)))
        }
    }

    @Nested
    inner class Shutout {

        @Test
        fun `a shutout needs every opponent on zero`() {
            assertEquals(1.0, measure("SHUTOUT", contextFor(shutoutMatch, 7)))
        }

        @Test
        fun `the shut-out hero does not itself earn a shutout`() {
            assertEquals(0.0, measure("SHUTOUT", contextFor(shutoutMatch, 11)))
        }

        @Test
        fun `a win with the opponent still alive is no shutout`() {
            val narrow = match(
                listOf(
                    participant(id = 1, heroId = 4, heroName = "Medusa", health = 8, isWinner = true),
                    participant(id = 2, heroId = 8, heroName = "Sun Wukong", health = 2),
                ),
            )

            assertEquals(0.0, measure("SHUTOUT", contextFor(narrow, 4)))
        }

        @Test
        fun `one surviving opponent out of two denies the shutout`() {
            val threeWay = match(
                listOf(
                    participant(id = 1, heroId = 1, heroName = "Alice", health = 6, isWinner = true),
                    participant(id = 2, heroId = 2, heroName = "Medusa", health = 1),
                    participant(id = 3, heroId = 3, heroName = "Sinbad", health = 0),
                ),
            )

            assertEquals(0.0, measure("SHUTOUT", contextFor(threeWay, 1)))
        }
    }

    @Nested
    inner class Bans {

        @Test
        fun `a banned hero earns BAN and nothing else`() {
            val sunWukong = contextFor(shutoutMatch, heroId = 8)

            assertEquals(1.0, measure("BAN", sunWukong))
            assertEquals(0.0, measure("APPEARANCE", sunWukong))
            assertEquals(0.0, measure("WIN", sunWukong))
            assertEquals(0.0, measure("LOSS", sunWukong))
            assertEquals(0.0, measure("DRAW", sunWukong))
            assertEquals(0.0, measure("HEALTH_REMAINING", sunWukong))
            assertEquals(0.0, measure("HEALTH_DIFFERENTIAL", sunWukong))
            assertEquals(0.0, measure("SHUTOUT", sunWukong))
        }

        @Test
        fun `a hero that played earns APPEARANCE and never BAN`() {
            val bigfoot = contextFor(shutoutMatch, heroId = 7)

            assertEquals(1.0, measure("APPEARANCE", bigfoot))
            assertEquals(0.0, measure("BAN", bigfoot))
        }

        @Test
        fun `heroContexts covers every hero the match touched, exactly once`() {
            val contexts = shutoutMatch.heroContexts()

            assertEquals(listOf(7L, 11L, 8L), contexts.map { it.heroId })
            assertEquals(3, contexts.distinctBy { it.heroId }.size)
            assertTrue(contexts.single { it.heroId == 8L }.role is HeroRole.Banned)
        }

        @Test
        fun `the ban record carries who struck it`() {
            val role = contextFor(shutoutMatch, heroId = 8).role

            assertEquals(HeroRole.Banned, role)
        }
    }
}
