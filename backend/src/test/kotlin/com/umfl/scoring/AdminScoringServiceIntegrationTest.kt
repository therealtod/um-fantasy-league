package com.umfl.scoring

import com.umfl.common.ConflictException
import com.umfl.common.ScoringRuleException
import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.TournamentRepository
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import java.math.BigDecimal
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class AdminScoringServiceIntegrationTest @Autowired constructor(
    private val adminScoringService: AdminScoringService,
    private val ruleSetRepository: ScoringRuleSetRepository,
    private val scoringRuleSetQuery: ScoringRuleSetQuery,
    private val tournamentRepository: TournamentRepository,
) : PostgresIntegrationTest() {

    private fun winterOfChampionsId() = requireNotNull(tournamentRepository.findByName("Winter of Champions")?.id)

    @Test
    fun `lists a tournament's rule sets, active one first`() {
        val tournamentId = winterOfChampionsId()
        adminScoringService.create(
            tournamentId,
            name = "Retuned Weights",
            coefficients = listOf(ScoringCoefficientInput("WIN", BigDecimal("12.0"), 0)),
            activate = false,
        )

        val listed = adminScoringService.list(tournamentId)

        assertEquals(2, listed.size)
        assertEquals("Season 2026 Standard", listed.first().ruleSet.name)
        assertTrue(listed.first().ruleSet.isActive)
        assertTrue(listed.any { it.ruleSet.name == "Retuned Weights" && !it.ruleSet.isActive })
    }

    @Test
    fun `listing surfaces the same unknown-metric warning as create does`() {
        val tournamentId = winterOfChampionsId()
        adminScoringService.create(
            tournamentId,
            name = "Experimental Weights",
            coefficients = listOf(ScoringCoefficientInput("CROWD_FAVOURITE", BigDecimal("5.0"), 0)),
            activate = false,
        )

        val listed = adminScoringService.list(tournamentId)

        assertEquals(listOf("CROWD_FAVOURITE"), listed.single { it.ruleSet.name == "Experimental Weights" }.unknownMetrics)
    }

    @Test
    fun `creates a new inactive rule set alongside the seeded active one`() {
        val tournamentId = winterOfChampionsId()

        val result = adminScoringService.create(
            tournamentId,
            name = "Retuned Weights",
            coefficients = listOf(ScoringCoefficientInput("WIN", BigDecimal("12.0"), 0)),
            activate = false,
        )

        assertFalse(result.ruleSet.isActive)
        assertEquals(1, result.ruleSet.coefficients.size)
        assertTrue(result.unknownMetrics.isEmpty())
        // The seeded rule set is still the one standings actually uses.
        assertEquals("Season 2026 Standard", scoringRuleSetQuery.activeRules(tournamentId).name)
    }

    @Test
    fun `creating with an unknown metric warns but does not reject`() {
        val tournamentId = winterOfChampionsId()

        val result = adminScoringService.create(
            tournamentId,
            name = "Experimental Weights",
            coefficients = listOf(ScoringCoefficientInput("CROWD_FAVOURITE", BigDecimal("5.0"), 0)),
            activate = false,
        )

        assertEquals(listOf("CROWD_FAVOURITE"), result.unknownMetrics)
    }

    @Test
    fun `creating a rule set with a name already used in this tournament is rejected`() {
        assertFailsWith<ConflictException> {
            adminScoringService.create(
                winterOfChampionsId(),
                name = "Season 2026 Standard",
                coefficients = listOf(ScoringCoefficientInput("WIN", BigDecimal.TEN, 0)),
                activate = false,
            )
        }
    }

    @Test
    fun `activating a new rule set deactivates the previously-active one`() {
        val tournamentId = winterOfChampionsId()
        val created = adminScoringService.create(
            tournamentId,
            name = "Retuned Weights",
            coefficients = listOf(ScoringCoefficientInput("WIN", BigDecimal("12.0"), 0)),
            activate = false,
        ).ruleSet

        adminScoringService.activate(tournamentId, requireNotNull(created.id))

        val seeded = requireNotNull(ruleSetRepository.findByTournamentIdAndName(tournamentId, "Season 2026 Standard"))
        assertFalse(seeded.isActive)
        assertEquals("Retuned Weights", scoringRuleSetQuery.activeRules(tournamentId).name)
        assertEquals(BigDecimal("12.0000"), scoringRuleSetQuery.activeRules(tournamentId).coefficientOf("WIN"))
    }

    @Test
    fun `create can activate immediately`() {
        val tournamentId = winterOfChampionsId()

        adminScoringService.create(
            tournamentId,
            name = "Immediate Activation",
            coefficients = listOf(ScoringCoefficientInput("WIN", BigDecimal("9.0"), 0)),
            activate = true,
        )

        assertEquals("Immediate Activation", scoringRuleSetQuery.activeRules(tournamentId).name)
    }

    @Test
    fun `updates a rule set's coefficients`() {
        val tournamentId = winterOfChampionsId()
        val created = adminScoringService.create(
            tournamentId,
            name = "Retuned Weights",
            coefficients = listOf(ScoringCoefficientInput("WIN", BigDecimal("12.0"), 0)),
            activate = false,
        ).ruleSet

        val updated = adminScoringService.update(
            tournamentId,
            requireNotNull(created.id),
            name = "Retuned Weights",
            coefficients = listOf(
                ScoringCoefficientInput("WIN", BigDecimal("15.0"), 0),
                ScoringCoefficientInput("BAN", BigDecimal("3.0"), 1),
            ),
        ).ruleSet

        assertEquals(2, updated.coefficients.size)
        assertTrue(updated.coefficients.any { it.metric == "BAN" })
    }

    /**
     * Both of these used to reach the database and come back as the
     * `DataIntegrityViolationException` backstop's generic 409 — the unique
     * `(rule_set_id, metric)` index for the first, the metric format CHECK for
     * the second. [ScoringRuleSetPolicy] now names them before the insert.
     */
    @Test
    fun `creating with the same metric twice is a named rule violation, not a data integrity 409`() {
        val failure = assertFailsWith<ScoringRuleException> {
            adminScoringService.create(
                winterOfChampionsId(),
                name = "Double Weighted",
                coefficients = listOf(
                    ScoringCoefficientInput("WIN", BigDecimal("1.0"), 0),
                    ScoringCoefficientInput("WIN", BigDecimal("2.0"), 1),
                ),
                activate = false,
            )
        }

        assertEquals(listOf(ScoringRule.DUPLICATE_METRIC), failure.violations.map { it.rule })
    }

    @Test
    fun `updating with a malformed metric is a named rule violation, not a data integrity 409`() {
        val tournamentId = winterOfChampionsId()
        val created = adminScoringService.create(
            tournamentId,
            name = "Retuned Weights",
            coefficients = listOf(ScoringCoefficientInput("WIN", BigDecimal("12.0"), 0)),
            activate = false,
        ).ruleSet

        val failure = assertFailsWith<ScoringRuleException> {
            adminScoringService.update(
                tournamentId,
                requireNotNull(created.id),
                name = "Retuned Weights",
                coefficients = listOf(ScoringCoefficientInput("win-rate", BigDecimal("1.0"), 0)),
            )
        }

        assertEquals(listOf(ScoringRule.MALFORMED_METRIC), failure.violations.map { it.rule })
    }
}
