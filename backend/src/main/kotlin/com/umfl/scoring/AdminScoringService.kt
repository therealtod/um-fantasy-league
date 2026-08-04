package com.umfl.scoring

import com.umfl.common.ConflictException
import com.umfl.common.NotFoundException
import com.umfl.common.ScoringRuleException
import com.umfl.tournament.TournamentService
import org.springframework.stereotype.Service
import org.springframework.transaction.annotation.Transactional

/** One rule set as create/update/list return it, paired with any metrics nothing prices — see [ScoringRuleSet]'s doc. */
data class ScoringRuleSetResult(val ruleSet: ScoringRuleSet, val unknownMetrics: List<String>)

@Service
class AdminScoringService(
    private val tournamentService: TournamentService,
    private val ruleSetRepository: ScoringRuleSetRepository,
) {

    /**
     * Every rule set for [tournamentId], active one first — an admin's only way
     * to see what already exists. Transactional despite being a read: Spring
     * Data JDBC loads each rule set's `coefficients` in its own statement, so
     * without one the listing could straddle a concurrent activate/update.
     */
    @Transactional(readOnly = true)
    fun list(tournamentId: Long): List<ScoringRuleSetResult> {
        tournamentService.requireTournament(tournamentId)
        return ruleSetRepository.findByTournamentId(tournamentId)
            .sortedWith(compareByDescending<ScoringRuleSet> { it.isActive }.thenBy { it.name })
            .map { ScoringRuleSetResult(it, MatchMetrics.unknown(it.coefficients.map { c -> c.metric })) }
    }

    @Transactional
    fun create(
        tournamentId: Long,
        name: String,
        coefficients: List<ScoringCoefficientInput>,
        activate: Boolean,
    ): ScoringRuleSetResult {
        tournamentService.requireTournament(tournamentId)
        validate(coefficients)
        if (ruleSetRepository.findByTournamentIdAndName(tournamentId, name) != null) {
            throw ConflictException("A scoring rule set named '$name' already exists for tournament $tournamentId.")
        }

        val saved = ruleSetRepository.save(
            ScoringRuleSet(
                tournamentId = tournamentId,
                name = name,
                isActive = false,
                coefficients = toCoefficients(coefficients),
            )
        )
        val result = if (activate) activate(tournamentId, requireNotNull(saved.id)) else saved
        return ScoringRuleSetResult(result, MatchMetrics.unknown(coefficients.map { it.metric }))
    }

    @Transactional
    fun update(
        tournamentId: Long,
        ruleSetId: Long,
        name: String,
        coefficients: List<ScoringCoefficientInput>,
    ): ScoringRuleSetResult {
        val existing = requireRuleSet(tournamentId, ruleSetId)
        validate(coefficients)
        val collision = ruleSetRepository.findByTournamentIdAndName(tournamentId, name)
        if (collision != null && collision.id != ruleSetId) {
            throw ConflictException("A scoring rule set named '$name' already exists for tournament $tournamentId.")
        }

        val saved = ruleSetRepository.save(
            existing.copy(name = name, coefficients = toCoefficients(coefficients)),
        )
        return ScoringRuleSetResult(saved, MatchMetrics.unknown(coefficients.map { it.metric }))
    }

    /**
     * Deactivates any currently-active sibling before activating [ruleSetId] —
     * two separate saves, so the partial unique index never sees two active
     * rows for the same tournament at once.
     */
    @Transactional
    fun activate(tournamentId: Long, ruleSetId: Long): ScoringRuleSet {
        val target = requireRuleSet(tournamentId, ruleSetId)

        ruleSetRepository.findByTournamentId(tournamentId)
            .filter { it.isActive && it.id != ruleSetId }
            .forEach { ruleSetRepository.save(it.copy(isActive = false)) }

        return ruleSetRepository.save(target.copy(isActive = true))
    }

    /**
     * Duplicate and malformed metrics are caught here rather than left to the
     * `unique (rule_set_id, metric)` constraint and the format CHECK, which
     * would surface as the generic 409 backstop with nothing naming the bad row.
     */
    private fun validate(coefficients: List<ScoringCoefficientInput>) {
        val violations = ScoringRuleSetPolicy.validate(coefficients)
        if (violations.isNotEmpty()) throw ScoringRuleException(violations)
    }

    private fun requireRuleSet(tournamentId: Long, ruleSetId: Long): ScoringRuleSet =
        ruleSetRepository.findById(ruleSetId)
            .filter { it.tournamentId == tournamentId }
            .orElseThrow { NotFoundException("No scoring rule set $ruleSetId for tournament $tournamentId") }

    private fun toCoefficients(coefficients: List<ScoringCoefficientInput>): Set<ScoringCoefficient> =
        coefficients.map {
            ScoringCoefficient(metric = MatchMetrics.normalise(it.metric), coefficient = it.coefficient, sortOrder = it.sortOrder)
        }.toSet()
}
