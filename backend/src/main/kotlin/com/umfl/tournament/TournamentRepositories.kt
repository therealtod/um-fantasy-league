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
     * Take a row lock on the tournament and return its *current* capacity.
     *
     * `capacity` has no database constraint behind it the way double
     * registration has `unique (tournament_id, manager_id)`, so the count-then-
     * insert in [TournamentService.register] needs the seat check and the insert
     * to be serialised against each other. Locking the tournament row — not the
     * entries — gives every concurrent registration for the same tournament one
     * queue to stand in, and costs nothing anywhere else: no other statement in
     * the app writes `tournaments` on the manager path.
     *
     * Returning capacity rather than just the id matters because the lock alone
     * only serialises the *write*: a concurrent admin capacity change
     * (`AdminTournamentService.update`) still commits and releases before this
     * transaction's `FOR UPDATE` is granted, but the `Tournament` [register]
     * already holds was read *before* that lock, so its `capacity` field is the
     * pre-update value. Re-reading capacity here, after the lock is granted, is
     * what makes the seat check exact instead of checking a stale snapshot.
     */
    @Query("select capacity from tournaments where id = :id for update")
    fun lockCapacityById(id: Long): Int?
}

@Repository
interface TournamentEntryRepository : CrudRepository<TournamentEntry, Long> {

    fun findByTournamentIdAndManagerId(tournamentId: Long, managerId: Long): TournamentEntry?

    fun findByTournamentId(tournamentId: Long): List<TournamentEntry>

    fun countByTournamentId(tournamentId: Long): Int

    /**
     * Enrolment counts for every tournament in one round trip, so the Lobby does
     * not issue a count query per card.
     */
    @Query(
        """
        select t.id as tournament_id, count(e.id) as entry_count
        from tournaments t
        left join tournament_entries e on e.tournament_id = t.id
        group by t.id
        """
    )
    fun countEntriesPerTournament(): List<TournamentEntryCount>
}

data class TournamentEntryCount(val tournamentId: Long, val entryCount: Int)
