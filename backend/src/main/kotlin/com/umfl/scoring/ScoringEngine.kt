package com.umfl.scoring

import java.math.BigDecimal
import java.math.RoundingMode

/**
 * One tournament's active scoring configuration, as read out of
 * `scoring_rule_set` / `scoring_coefficient`.
 *
 * [coefficients] is insertion-ordered by `sort_order`, which is what fixes the
 * leaderboard's left-to-right column order -- the backend cannot know it any
 * other way.
 */
data class ScoringRules(
    val ruleSetId: Long,
    val name: String,
    val coefficients: Map<String, BigDecimal>,
) {
    /**
     * Metric keys are matched case- and whitespace-insensitively, so `' win '`
     * and `'Win'` cannot become separate columns. (The schema's CHECK already
     * guards against that; this makes the domain object safe on its own.)
     */
    private val byMetric: Map<String, BigDecimal> =
        coefficients.entries.associate { (metric, coefficient) ->
            MatchMetrics.normalise(metric) to coefficient
        }

    /** The metrics that actually score: known to [MatchMetrics], in column order. */
    val scoredMetrics: List<String> = byMetric.keys.filter { MatchMetrics[it] != null }

    /** Configured metrics nothing implements. Worth logging once; never thrown. */
    val unknownMetrics: List<String> = MatchMetrics.unknown(coefficients.keys)

    fun coefficientOf(metric: String): BigDecimal =
        byMetric[MatchMetrics.normalise(metric)] ?: BigDecimal.ZERO

    companion object {
        /** A tournament with no active rule set: everything scores zero. */
        val NONE = ScoringRules(ruleSetId = 0, name = "", coefficients = emptyMap())
    }
}

/**
 * Prices a hero's involvement in a match against a set of coefficients.
 *
 * A pure object with no Spring and no persistence: the weights are data now, so
 * there is nothing left to inject. Points are computed at read time and never
 * stored -- coefficients are mutable reference data, so a stored total would be
 * a cache with nothing to invalidate it.
 */
object ScoringEngine {

    /**
     * Each metric's contribution, keyed by metric, in column order.
     *
     * Zero contributions are omitted: a metric the hero did not earn (or one
     * weighted at zero) is absent rather than present as `0.0`. Each value is
     * rounded to 2dp *before* it is summed, so a displayed total is exactly the
     * sum of its displayed parts.
     */
    fun breakdown(context: MetricContext, rules: ScoringRules): Map<String, Double> {
        if (rules.scoredMetrics.isEmpty()) return emptyMap()
        return buildMap {
            for (metric in rules.scoredMetrics) {
                val extractor = MatchMetrics[metric] ?: continue
                val measured = extractor(context)
                if (measured == 0.0) continue
                val points = round2(measured * rules.coefficientOf(metric).toDouble())
                if (points != 0.0) put(metric, points)
            }
        }
    }

    /** This hero's net score for this match -- may be negative. */
    fun score(context: MetricContext, rules: ScoringRules): Double =
        round2(breakdown(context, rules).values.sum())

    /** Half-up to 2dp, so the arithmetic matches what the leaderboard prints. */
    fun round2(value: Double): Double =
        BigDecimal.valueOf(value).setScale(2, RoundingMode.HALF_UP).toDouble()
}
