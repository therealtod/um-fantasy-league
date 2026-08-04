package com.umfl.scoring

import java.math.BigDecimal

enum class ScoringRule {
    /** Two coefficients in the same rule set price the same metric. */
    DUPLICATE_METRIC,

    /** A metric name is not SCREAMING_SNAKE_CASE, so the schema's format CHECK would reject it. */
    MALFORMED_METRIC,
}

data class ScoringViolation(val rule: ScoringRule, val message: String)

data class ScoringCoefficientInput(val metric: String, val coefficient: BigDecimal, val sortOrder: Int = 0)

/**
 * Pre-validates a scoring rule set's coefficients before the service saves
 * them, so a bad admin submission comes back as a named 422 instead of the
 * generic 409 a `unique (rule_set_id, metric)` or format-CHECK failure would
 * produce further down.
 *
 * Deliberately free of Spring and persistence, exactly like
 * [com.umfl.match.MatchResultPolicy] and [com.umfl.tournament.RosterPolicy].
 *
 * It validates the *shape* of a metric name, never the *set* of legal names: a
 * metric no extractor implements is a deliberate non-blocking warning
 * ([MatchMetrics.unknown]), not a rejection, and must stay one.
 */
object ScoringRuleSetPolicy {

    /** Mirrors the `scoring_coefficient_metric_format` CHECK in `V1__core_schema.sql`. */
    private val METRIC_FORMAT = Regex("^[A-Z][A-Z0-9_]*\$")

    /**
     * Both checks run against the *normalised* metric, because that is what
     * gets stored — `' win '` and `'Win'` are the same column, so they are also
     * a duplicate of each other.
     */
    fun validate(coefficients: List<ScoringCoefficientInput>): List<ScoringViolation> = buildList {
        val metrics = coefficients.map { MatchMetrics.normalise(it.metric) }

        val malformed = metrics.filterNot(METRIC_FORMAT::matches).distinct()
        if (malformed.isNotEmpty()) {
            add(
                ScoringViolation(
                    ScoringRule.MALFORMED_METRIC,
                    "Metric name(s) must be letters, digits and underscores starting with a letter: " +
                        malformed.joinToString { "'$it'" } + ".",
                )
            )
        }

        val duplicates = metrics.groupingBy { it }.eachCount().filterValues { it > 1 }.keys
        if (duplicates.isNotEmpty()) {
            add(
                ScoringViolation(
                    ScoringRule.DUPLICATE_METRIC,
                    "Metric(s) priced more than once: ${duplicates.sorted().joinToString { "'$it'" }}.",
                )
            )
        }
    }
}
