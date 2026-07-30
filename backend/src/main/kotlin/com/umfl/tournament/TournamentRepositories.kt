package com.umfl.tournament

import org.springframework.data.jdbc.repository.query.Query
import org.springframework.data.repository.CrudRepository
import org.springframework.stereotype.Repository

@Repository
interface TournamentRepository : CrudRepository<Tournament, Long> {
    fun findAllByOrderByStartDateAsc(): List<Tournament>
    fun findByStatusOrderByStartDateAsc(status: TournamentStatus): List<Tournament>
    fun findByName(name: String): Tournament?
}

@Repository
interface TournamentEntryRepository : CrudRepository<TournamentEntry, Long> {

    fun findByTournamentIdAndManagerId(tournamentId: Long, managerId: Long): TournamentEntry?

    fun findByTournamentId(tournamentId: Long): List<TournamentEntry>

    fun findByManagerId(managerId: Long): List<TournamentEntry>

    fun countByTournamentId(tournamentId: Long): Int

    /**
     * Enrolment counts for every tournament in one round trip, so the Lobby does
     * not issue a count query per card.
     */
    @Query(
        """
        select t.id as tournament_id, count(e.id) as entry_count
        from tournament t
        left join tournament_entry e on e.tournament_id = t.id
        group by t.id
        """
    )
    fun countEntriesPerTournament(): List<TournamentEntryCount>
}

data class TournamentEntryCount(val tournamentId: Long, val entryCount: Int)
