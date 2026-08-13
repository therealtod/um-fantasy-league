package com.umfl.tournament

import org.springframework.jdbc.core.simple.JdbcClient
import org.springframework.stereotype.Repository

/**
 * Read-side projections over `tournament_entry`.
 *
 * The Lobby only ever asks "which of these tournaments am I in, and is my roster
 * locked?". Loading [TournamentEntry] aggregates to answer that costs a second
 * query per entry to populate `entry_slot`, so the status lives here as a
 * projection instead — the same read/write split the rest of the app uses.
 */
@Repository
class TournamentEntryQuery(private val jdbcClient: JdbcClient) {

    /**
     * Entry status per tournament for one manager, backed by
     * `idx_tournament_entry_manager`.
     */
    fun statusesByTournament(managerId: Long): Map<Long, EntryStatus> =
        jdbcClient
            .sql("select tournament_id, status from tournament_entry where manager_id = :managerId")
            .param("managerId", managerId)
            .query { rs, _ -> rs.getLong("tournament_id") to EntryStatus.valueOf(rs.getString("status")) }
            .list()
            .toMap()

    /** Entry status for one manager in one tournament, backed by the same index. */
    fun statusFor(managerId: Long, tournamentId: Long): EntryStatus? =
        jdbcClient
            .sql("select status from tournament_entry where manager_id = :managerId and tournament_id = :tournamentId")
            .param("managerId", managerId)
            .param("tournamentId", tournamentId)
            .query { rs, _ -> EntryStatus.valueOf(rs.getString("status")) }
            .optional()
            .orElse(null)
}
