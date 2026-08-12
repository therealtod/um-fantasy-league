package com.umfl.scoring

import com.umfl.match.BanResult
import com.umfl.match.BanType
import com.umfl.match.GameParticipantResult
import com.umfl.match.GameResult
import com.umfl.match.MatchParticipantResult
import com.umfl.match.MatchResult
import org.junit.jupiter.api.Test
import java.math.BigDecimal
import java.time.Instant
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * The fold from measured metrics to points.
 *
 * The coefficients used here are the seeded "Season 2026 Standard" weights,
 * including the deliberately unimplemented `CROWD_FAVOURITE` — proving in a
 * plain unit test what [com.umfl.standings.StandingsIntegrationTest] then proves
 * against the real database.
 */
class ScoringEngineTest {

    private fun rules(vararg weights: Pair<String, String>) = ScoringRules(
        ruleSetId = 1,
        name = "Season 2026 Standard",
        coefficients = linkedMapOf(*weights.map { it.first to BigDecimal(it.second) }.toTypedArray()),
    )

    private val standard = rules(
        "WIN" to "10.0000",
        "HEALTH_REMAINING" to "0.7500",
        "HEALTH_DIFFERENTIAL" to "0.5000",
        "SHUTOUT" to "3.0000",
        "BAN" to "2.0000",
        "APPEARANCE" to "1.0000",
        "CROWD_FAVOURITE" to "5.0000",
    )

    /** Match 6 of the seed: Bigfoot 11 shuts out Beowulf 0, Sun Wukong banned. */
    private val match = MatchResult(
        matchId = 6,
        tournamentId = 1,
        round = 2,
        playedAt = Instant.parse("2026-06-06T11:00:00Z"),
        externalLink = null,
        participants = listOf(
            MatchParticipantResult(side = 0, playerLabel = "Aurelie Blanc"),
            MatchParticipantResult(side = 1, playerLabel = "Miles Ashworth"),
        ),
        games = listOf(
            GameResult(
                gameId = 1,
                gameNumber = 1,
                mapId = 3,
                mapName = "Raptor Paddock",
                participants = listOf(
                    GameParticipantResult(side = 0, heroId = 7, heroName = "Bigfoot", healthRemaining = 11, isWinner = true),
                    GameParticipantResult(side = 1, heroId = 11, heroName = "Beowulf", healthRemaining = 0, isWinner = false),
                ),
            ),
        ),
        bans = listOf(BanResult(heroId = 8, heroName = "Sun Wukong", banType = BanType.PRE_BAN)),
    )

    private fun contextFor(heroId: Long) = match.heroContexts().single { it.heroId == heroId }

    @Test
    fun `a winning hero banks every metric it earned`() {
        val breakdown = ScoringEngine.breakdown(contextFor(7), standard)

        assertEquals(
            mapOf(
                "WIN" to 10.0,
                "HEALTH_REMAINING" to 8.25, // 11 * 0.75
                "HEALTH_DIFFERENTIAL" to 5.5, // (11 - 0) * 0.5
                "SHUTOUT" to 3.0,
                "APPEARANCE" to 1.0,
            ),
            breakdown,
        )
        assertEquals(27.75, ScoringEngine.score(contextFor(7), standard))
    }

    @Test
    fun `the total is exactly the sum of the breakdown`() {
        for (heroId in listOf(7L, 11L, 8L)) {
            val breakdown = ScoringEngine.breakdown(contextFor(heroId), standard)
            assertEquals(
                ScoringEngine.round2(breakdown.values.sum()),
                ScoringEngine.score(contextFor(heroId), standard),
                "hero $heroId",
            )
        }
    }

    @Test
    fun `an unknown metric contributes nothing and does not throw`() {
        assertFalse("CROWD_FAVOURITE" in ScoringEngine.breakdown(contextFor(7), standard))
        assertEquals(listOf("CROWD_FAVOURITE"), standard.unknownMetrics)
        assertFalse("CROWD_FAVOURITE" in standard.scoredMetrics)
    }

    @Test
    fun `scored metrics keep their configured column order`() {
        assertEquals(
            listOf("WIN", "HEALTH_REMAINING", "HEALTH_DIFFERENTIAL", "SHUTOUT", "BAN", "APPEARANCE"),
            standard.scoredMetrics,
        )
    }

    @Test
    fun `a zero coefficient omits the entry rather than showing a zero`() {
        val noAppearanceBonus = rules("WIN" to "10.0000", "APPEARANCE" to "0.0000")

        val breakdown = ScoringEngine.breakdown(contextFor(7), noAppearanceBonus)

        assertEquals(mapOf("WIN" to 10.0), breakdown)
        assertTrue("APPEARANCE" in noAppearanceBonus.scoredMetrics, "it is still a column, just a worthless one")
    }

    @Test
    fun `a metric the hero did not earn is absent, not zero`() {
        val breakdown = ScoringEngine.breakdown(contextFor(11), standard)

        assertEquals(mapOf("APPEARANCE" to 1.0, "HEALTH_DIFFERENTIAL" to -5.5), breakdown)
        assertFalse("WIN" in breakdown)
        assertFalse("SHUTOUT" in breakdown)
        assertFalse("HEALTH_REMAINING" in breakdown, "zero health times any weight is still zero")
    }

    @Test
    fun `a banned hero scores only its ban`() {
        val breakdown = ScoringEngine.breakdown(contextFor(8), standard)

        assertEquals(mapOf("BAN" to 2.0), breakdown)
        assertEquals(2.0, ScoringEngine.score(contextFor(8), standard))
    }

    @Test
    fun `negative coefficients are legitimate penalties`() {
        val penalised = rules("HEALTH_DIFFERENTIAL" to "0.5000", "LOSS" to "-6.0000")

        assertEquals(mapOf("HEALTH_DIFFERENTIAL" to -5.5, "LOSS" to -6.0), ScoringEngine.breakdown(contextFor(11), penalised))
        assertEquals(-11.5, ScoringEngine.score(contextFor(11), penalised))
    }

    @Test
    fun `metric keys are matched case- and whitespace-insensitively`() {
        val sloppy = rules(" win " to "10.0000", "Appearance" to "1.0000")

        assertEquals(listOf("WIN", "APPEARANCE"), sloppy.scoredMetrics)
        assertEquals(mapOf("WIN" to 10.0, "APPEARANCE" to 1.0), ScoringEngine.breakdown(contextFor(7), sloppy))
        assertTrue(sloppy.unknownMetrics.isEmpty())
    }

    @Test
    fun `each contribution is rounded before it is summed`() {
        // 11 health at 0.333 is 3.663, which must round to 3.66 before joining the total.
        val awkward = rules("HEALTH_REMAINING" to "0.3330")

        assertEquals(mapOf("HEALTH_REMAINING" to 3.66), ScoringEngine.breakdown(contextFor(7), awkward))
        assertEquals(3.66, ScoringEngine.score(contextFor(7), awkward))
    }

    @Test
    fun `a tournament with no active rule set scores nothing`() {
        assertEquals(emptyMap<String, Double>(), ScoringEngine.breakdown(contextFor(7), ScoringRules.NONE))
        assertEquals(0.0, ScoringEngine.score(contextFor(7), ScoringRules.NONE))
        assertEquals("", ScoringRules.NONE.name)
        assertTrue(ScoringRules.NONE.scoredMetrics.isEmpty())
    }
}
