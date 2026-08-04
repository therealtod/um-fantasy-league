package com.umfl.scoring

import org.junit.jupiter.api.Test
import java.math.BigDecimal
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ScoringRuleSetPolicyTest {

    private fun coefficient(metric: String, weight: String = "1.0", sortOrder: Int = 0) =
        ScoringCoefficientInput(metric = metric, coefficient = BigDecimal(weight), sortOrder = sortOrder)

    @Test
    fun `a well-formed rule set has no violations`() {
        val violations = ScoringRuleSetPolicy.validate(
            listOf(coefficient("WIN", "3.0"), coefficient("HEALTH_REMAINING", "0.5", sortOrder = 1)),
        )

        assertTrue(violations.isEmpty(), "expected no violations but got $violations")
    }

    @Test
    fun `the same metric priced twice is rejected`() {
        val violations = ScoringRuleSetPolicy.validate(listOf(coefficient("WIN", "1.0"), coefficient("WIN", "2.0")))

        assertEquals(listOf(ScoringRule.DUPLICATE_METRIC), violations.map { it.rule })
        assertTrue(violations.single().message.contains("WIN"))
    }

    /** `normalise` folds these to the same stored column, so they are duplicates of each other. */
    @Test
    fun `metrics that differ only in case or padding are duplicates`() {
        val violations = ScoringRuleSetPolicy.validate(listOf(coefficient(" win "), coefficient("Win")))

        assertEquals(listOf(ScoringRule.DUPLICATE_METRIC), violations.map { it.rule })
    }

    @Test
    fun `a hyphenated metric is rejected rather than reaching the schema's format check`() {
        val violations = ScoringRuleSetPolicy.validate(listOf(coefficient("win-rate")))

        assertEquals(listOf(ScoringRule.MALFORMED_METRIC), violations.map { it.rule })
        assertTrue(violations.single().message.contains("WIN-RATE"))
    }

    @Test
    fun `a metric starting with a digit is rejected`() {
        val violations = ScoringRuleSetPolicy.validate(listOf(coefficient("1st")))

        assertEquals(listOf(ScoringRule.MALFORMED_METRIC), violations.map { it.rule })
    }

    @Test
    fun `a blank metric is rejected`() {
        val violations = ScoringRuleSetPolicy.validate(listOf(coefficient("   ")))

        assertEquals(listOf(ScoringRule.MALFORMED_METRIC), violations.map { it.rule })
    }

    @Test
    fun `digits and underscores after the first letter are legal`() {
        val violations = ScoringRuleSetPolicy.validate(listOf(coefficient("BEST_OF_3")))

        assertTrue(violations.isEmpty(), "expected no violations but got $violations")
    }

    /**
     * The registry deliberately ignores metrics it can't price — that is a
     * warning on the response, never a rejection. This is the guard against
     * someone turning [ScoringRuleSetPolicy] into a whitelist.
     */
    @Test
    fun `a metric no extractor implements is legal`() {
        val violations = ScoringRuleSetPolicy.validate(listOf(coefficient("CROWD_FAVOURITE")))

        assertTrue(violations.isEmpty(), "expected no violations but got $violations")
        assertEquals(listOf("CROWD_FAVOURITE"), MatchMetrics.unknown(listOf("CROWD_FAVOURITE")))
    }

    /** A negative weight is a penalty, which the schema deliberately allows. */
    @Test
    fun `a negative coefficient is legal`() {
        val violations = ScoringRuleSetPolicy.validate(listOf(coefficient("LOSS", "-1.5")))

        assertTrue(violations.isEmpty(), "expected no violations but got $violations")
    }

    @Test
    fun `every broken rule is reported, not just the first`() {
        val violations = ScoringRuleSetPolicy.validate(
            listOf(coefficient("win-rate"), coefficient("WIN"), coefficient("win")),
        )

        assertEquals(
            setOf(ScoringRule.MALFORMED_METRIC, ScoringRule.DUPLICATE_METRIC),
            violations.map { it.rule }.toSet(),
        )
    }
}
