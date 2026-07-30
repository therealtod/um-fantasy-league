package com.umfl.scoring

import org.springframework.data.repository.CrudRepository
import org.springframework.stereotype.Repository

@Repository
interface ScoringRuleSetRepository : CrudRepository<ScoringRuleSet, Long> {
    fun findByTournamentId(tournamentId: Long): List<ScoringRuleSet>
    fun findByTournamentIdAndName(tournamentId: Long, name: String): ScoringRuleSet?
}
