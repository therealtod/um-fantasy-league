package com.umfl.tournament

import org.springframework.data.jdbc.repository.query.Query
import org.springframework.data.repository.CrudRepository
import org.springframework.stereotype.Repository

@Repository
interface TournamentRepository : CrudRepository<Tournament, Long> {
    fun findAllByOrderByStartDateAsc(): List<Tournament>
    fun findByStatusOrderByStartDateAsc(status: TournamentStatus): List<Tournament>
    fun findByName(name: String): Tournament?

    /**
     * Take a row lock on the tournament and return its id.
     *
     * `capacity` has no database constraint behind it the way double
     * registration has `unique (tournament_id, manager_id)`, so the count-then-
     * insert in [TournamentService.register] needs the seat check and the insert
     * to be serialised against each other. Locking the tournament row — not the
     * entries — gives every concurrent registration for the same tournament one
     * queue to stand in, and costs nothing anywhere else: no other statement in
     * the app writes `tournament` on the manager path.
     *
     * Selecting only the id keeps this a lock acquisition rather than a second
     * load of an aggregate the caller already has.
     */
    @Query("select id from tournament where id = :id for update")
    fun lockById(id: Long): Long?
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
