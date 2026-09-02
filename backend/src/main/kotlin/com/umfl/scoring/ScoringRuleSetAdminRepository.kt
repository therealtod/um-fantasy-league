package com.umfl.scoring

import org.springframework.jdbc.core.simple.JdbcClient
import org.springframework.stereotype.Repository

/**
 * Targeted `is_active` writes for `scoring_rule_sets`.
 *
 * [ScoringRuleSet] is an aggregate root owning `coefficients` via
 * `@MappedCollection`, so saving it through [ScoringRuleSetRepository] deletes
 * and reinserts every child row — a whole coefficient table churn (and burnt
 * sequence values) just to flip one boolean, on *both* the outgoing and the
 * incoming rule set. Activation touches no child data, so it goes through
 * `JdbcClient` instead, the same read/write split
 * [com.umfl.hero.HeroPoolAdminRepository] uses.
 */
@Repository
class ScoringRuleSetAdminRepository(private val jdbcClient: JdbcClient) {

    /**
     * Clears `is_active` on every rule set of [tournamentId] except
     * [exceptRuleSetId]. Must run before [activate] so the partial unique index
     * (`uq_scoring_rule_set_active`) never sees two active rows at once.
     */
    fun deactivateOthers(tournamentId: Long, exceptRuleSetId: Long) {
        jdbcClient.sql(
            """
            update scoring_rule_sets
               set is_active = false
             where tournament_id = :tournamentId
               and id <> :exceptRuleSetId
               and is_active
            """
        ).param("tournamentId", tournamentId).param("exceptRuleSetId", exceptRuleSetId).update()
    }

    fun activate(ruleSetId: Long) {
        jdbcClient.sql("update scoring_rule_sets set is_active = true where id = :ruleSetId")
            .param("ruleSetId", ruleSetId).update()
    }
}
