package com.umfl.scoring

import org.springframework.data.annotation.Id
import org.springframework.data.relational.core.mapping.MappedCollection
import org.springframework.data.relational.core.mapping.Table
import java.math.BigDecimal

/**
 * A tournament's scoring configuration — the admin write side of what
 * [ScoringRuleSetQuery] reads. Saved as one aggregate, coefficients included,
 * the same "replace the whole child collection" pattern
 * [com.umfl.tournament.TournamentEntry] uses for its slots.
 *
 * At most one rule set may be active per tournament, enforced by a partial
 * unique index (`uq_scoring_rule_set_active`) — activating one is therefore a
 * service-level operation (see `AdminScoringService.activate`), not just a
 * field flip on this class.
 */
@Table("scoring_rule_set")
data class ScoringRuleSet(
    @Id val id: Long? = null,
    val tournamentId: Long,
    val name: String,
    val isActive: Boolean = false,
    @MappedCollection(idColumn = "rule_set_id")
    val coefficients: Set<ScoringCoefficient> = emptySet(),
)

@Table("scoring_coefficient")
data class ScoringCoefficient(
    @Id val id: Long? = null,
    val metric: String,
    val coefficient: BigDecimal,
    /** Fixes the leaderboard's left-to-right column order — app data, not a Spring Data JDBC list index. */
    val sortOrder: Int = 0,
)
