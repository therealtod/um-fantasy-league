package com.umfl.scoring

import com.umfl.match.BanResult
import com.umfl.match.BanType
import com.umfl.match.DraftedHeroResult
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
        "SELF_BAN" to "2.0000",
        "OPPONENT_BAN" to "2.0000",
        "APPEARANCE" to "1.0000",
        "CROWD_FAVOURITE" to "5.0000",
    )

    /** Match 6 of the seed: Bigfoot 11 shuts out Beowulf 0, Sun Wukong opponent-banned. */
    private val match = MatchResult(
        matchId = 6,
        tournamentId = 1,
        round = 2,
        playedAt = Instant.parse("2026-06-06T11:00:00Z"),
        externalLink = "https://example.com/match/scoring",
        participants = listOf(
            MatchParticipantResult(
                side = 0,
                playerLabel = "Aurelie Blanc",
                draftedHeroes = listOf(DraftedHeroResult(heroId = 7, heroName = "Bigfoot")),
            ),
            MatchParticipantResult(
                side = 1,
                playerLabel = "Miles Ashworth",
                draftedHeroes = listOf(DraftedHeroResult(heroId = 11, heroName = "Beowulf")),
            ),
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
        bans = listOf(BanResult(heroId = 8, heroName = "Sun Wukong", banType = BanType.OPPONENT_BAN)),
    )

    /** The hero's per-game context — where every metric but APPEARANCE is priced. */
    private fun contextFor(heroId: Long) =
        match.heroContexts().single { it.heroId == heroId && it.role !is HeroRole.Drafted }

    /** The hero's once-per-series draft context — where APPEARANCE is priced. */
    private fun draftContextFor(heroId: Long) =
        match.heroContexts().single { it.heroId == heroId && it.role is HeroRole.Drafted }

    @Test
    fun `a winning hero banks every metric its game earned`() {
        val breakdown = ScoringEngine.breakdown(contextFor(7), standard)

        assertEquals(
            mapOf(
                "WIN" to 10.0,
                "HEALTH_REMAINING" to 8.25, // 11 * 0.75
                "HEALTH_DIFFERENTIAL" to 5.5, // (11 - 0) * 0.5
                "SHUTOUT" to 3.0,
            ),
            breakdown,
        )
        assertEquals(26.75, ScoringEngine.score(contextFor(7), standard))
    }

    @Test
    fun `the appearance point rides on the draft, so the hero's match total is game plus draft`() {
        assertEquals(mapOf("APPEARANCE" to 1.0), ScoringEngine.breakdown(draftContextFor(7), standard))

        val matchTotal = match.heroContexts()
            .filter { it.heroId == 7L }
            .sumOf { ScoringEngine.score(it, standard) }
        assertEquals(27.75, matchTotal, "26.75 from the game it won, 1.00 for having been drafted")
    }

    @Test
    fun `the total is exactly the sum of the breakdown`() {
        for (context in match.heroContexts()) {
            val breakdown = ScoringEngine.breakdown(context, standard)
            assertEquals(
                ScoringEngine.round2(breakdown.values.sum()),
                ScoringEngine.score(context, standard),
                "hero ${context.heroId} as ${context.role}",
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
            listOf(
                "WIN", "HEALTH_REMAINING", "HEALTH_DIFFERENTIAL", "SHUTOUT",
                "SELF_BAN", "OPPONENT_BAN", "APPEARANCE",
            ),
            standard.scoredMetrics,
        )
    }

    @Test
    fun `a zero coefficient omits the entry rather than showing a zero`() {
        val noAppearanceBonus = rules("WIN" to "10.0000", "APPEARANCE" to "0.0000")

        assertEquals(mapOf("WIN" to 10.0), ScoringEngine.breakdown(contextFor(7), noAppearanceBonus))
        assertEquals(emptyMap(), ScoringEngine.breakdown(draftContextFor(7), noAppearanceBonus))
        assertTrue("APPEARANCE" in noAppearanceBonus.scoredMetrics, "it is still a column, just a worthless one")
    }

    @Test
    fun `a metric the hero did not earn is absent, not zero`() {
        val breakdown = ScoringEngine.breakdown(contextFor(11), standard)

        assertEquals(emptyMap(), breakdown, "a shut-out loser earns nothing the standard rules price")
        assertFalse("WIN" in breakdown)
        assertFalse("SHUTOUT" in breakdown)
        assertFalse("HEALTH_REMAINING" in breakdown, "zero health times any weight is still zero")
        assertFalse("HEALTH_DIFFERENTIAL" in breakdown, "the losing side has no differential to price")
        assertEquals(
            mapOf("APPEARANCE" to 1.0),
            ScoringEngine.breakdown(draftContextFor(11), standard),
            "it was drafted, which is what the match still owes it",
        )
    }

    @Test
    fun `a banned hero scores only its ban`() {
        val breakdown = ScoringEngine.breakdown(contextFor(8), standard)

        assertEquals(mapOf("OPPONENT_BAN" to 2.0), breakdown)
        assertEquals(2.0, ScoringEngine.score(contextFor(8), standard))
    }

    @Test
    fun `negative coefficients are legitimate penalties`() {
        val penalised = rules("HEALTH_DIFFERENTIAL" to "0.5000", "LOSS" to "-6.0000")

        assertEquals(mapOf("LOSS" to -6.0), ScoringEngine.breakdown(contextFor(11), penalised))
        assertEquals(-6.0, ScoringEngine.score(contextFor(11), penalised))
    }

    @Test
    fun `HEALTH_DIFFERENTIAL_TWO_WAY carries a negative contribution all the way to the total`() {
        // The same weight an admin would give the win-gated key, on the ungated one:
        // the shut-out loser is charged exactly what the winner is paid.
        val twoWay = rules("HEALTH_DIFFERENTIAL_TWO_WAY" to "0.5000")

        assertEquals(mapOf("HEALTH_DIFFERENTIAL_TWO_WAY" to 5.5), ScoringEngine.breakdown(contextFor(7), twoWay))
        assertEquals(5.5, ScoringEngine.score(contextFor(7), twoWay))

        assertEquals(mapOf("HEALTH_DIFFERENTIAL_TWO_WAY" to -5.5), ScoringEngine.breakdown(contextFor(11), twoWay))
        assertEquals(-5.5, ScoringEngine.score(contextFor(11), twoWay))

        assertTrue(twoWay.unknownMetrics.isEmpty(), "the registry implements it, so it is a column and not a warning")
    }

    @Test
    fun `the two differential keys are independent columns an admin can price separately`() {
        val both = rules("HEALTH_DIFFERENTIAL" to "0.5000", "HEALTH_DIFFERENTIAL_TWO_WAY" to "0.5000")

        assertEquals(listOf("HEALTH_DIFFERENTIAL", "HEALTH_DIFFERENTIAL_TWO_WAY"), both.scoredMetrics)
        assertEquals(
            mapOf("HEALTH_DIFFERENTIAL" to 5.5, "HEALTH_DIFFERENTIAL_TWO_WAY" to 5.5),
            ScoringEngine.breakdown(contextFor(7), both),
            "the winner earns both, since they measure the same gap",
        )
        assertEquals(
            mapOf("HEALTH_DIFFERENTIAL_TWO_WAY" to -5.5),
            ScoringEngine.breakdown(contextFor(11), both),
            "only the ungated key reaches the loser",
        )
    }

    @Test
    fun `metric keys are matched case- and whitespace-insensitively`() {
        val sloppy = rules(" win " to "10.0000", "Appearance" to "1.0000")

        assertEquals(listOf("WIN", "APPEARANCE"), sloppy.scoredMetrics)
        assertEquals(mapOf("WIN" to 10.0), ScoringEngine.breakdown(contextFor(7), sloppy))
        assertEquals(mapOf("APPEARANCE" to 1.0), ScoringEngine.breakdown(draftContextFor(7), sloppy))
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
