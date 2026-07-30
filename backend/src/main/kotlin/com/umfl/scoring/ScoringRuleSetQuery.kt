package com.umfl.scoring

import org.springframework.jdbc.core.simple.JdbcClient
import org.springframework.stereotype.Repository
import java.math.BigDecimal

/**
 * Loads a tournament's active scoring configuration.
 *
 * At most one rule set per tournament can be active — enforced by a partial
 * unique index, not by this query. A tournament with none scores zero, which is
 * a legitimate state (a freshly created event), so it returns
 * [ScoringRules.NONE] rather than throwing.
 */
@Repository
class ScoringRuleSetQuery(private val jdbcClient: JdbcClient) {

    fun activeRules(tournamentId: Long): ScoringRules {
        val rows = jdbcClient
            .sql(ACTIVE_RULES_SQL)
            .param("tournamentId", tournamentId)
            .query { rs, _ ->
                RuleRow(
                    ruleSetId = rs.getLong("rule_set_id"),
                    name = rs.getString("name"),
                    metric = rs.getString("metric"),
                    coefficient = rs.getBigDecimal("coefficient"),
                )
            }
            .list()

        val header = rows.firstOrNull() ?: return ScoringRules.NONE
        // linkedMapOf keeps sort_order, which is the leaderboard's column order.
        val coefficients = linkedMapOf<String, BigDecimal>()
        for (row in rows) {
            val metric = row.metric ?: continue
            coefficients[MatchMetrics.normalise(metric)] = row.coefficient ?: BigDecimal.ZERO
        }
        return ScoringRules(ruleSetId = header.ruleSetId, name = header.name, coefficients = coefficients)
    }

    private data class RuleRow(
        val ruleSetId: Long,
        val name: String,
        /** Null when the rule set exists but has no coefficients yet (left join). */
        val metric: String?,
        val coefficient: BigDecimal?,
    )

    private companion object {
        const val ACTIVE_RULES_SQL = """
            select rs.id as rule_set_id, rs.name, c.metric, c.coefficient
            from scoring_rule_set rs
            left join scoring_coefficient c on c.rule_set_id = rs.id
            where rs.tournament_id = :tournamentId
              and rs.is_active
            order by c.sort_order, c.metric
        """
    }
}
