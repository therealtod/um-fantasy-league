package com.umfl.tournament

import com.umfl.common.ConflictException
import com.umfl.match.MatchResultCache
import org.springframework.stereotype.Service
import org.springframework.transaction.annotation.Transactional
import java.time.LocalDate

@Service
class AdminTournamentService(
    private val tournamentRepository: TournamentRepository,
    private val tournamentService: TournamentService,
    private val matchResultCache: MatchResultCache,
) {

    @Transactional
    fun create(
        name: String,
        format: TournamentFormat,
        status: TournamentStatus,
        startDate: LocalDate,
        endDate: LocalDate?,
        capacity: Int,
        rosterSize: Int,
        creditGrant: Int,
    ): Tournament {
        if (tournamentRepository.findByName(name) != null) {
            throw ConflictException("A tournament named '$name' already exists.")
        }
        return tournamentRepository.save(
            Tournament(
                name = name,
                format = format,
                status = status,
                startDate = startDate,
                endDate = endDate,
                capacity = capacity,
                rosterSize = rosterSize,
                creditGrant = creditGrant,
            )
        )
    }

    /**
     * Full replace, including [status] — there is no status-transition state
     * machine: an admin is trusted to move a tournament through its lifecycle
     * sensibly.
     */
    @Transactional
    fun update(
        tournamentId: Long,
        name: String,
        format: TournamentFormat,
        status: TournamentStatus,
        startDate: LocalDate,
        endDate: LocalDate?,
        capacity: Int,
        rosterSize: Int,
        creditGrant: Int,
    ): Tournament {
        val existing = tournamentService.requireTournament(tournamentId)
        val collision = tournamentRepository.findByName(name)
        if (collision != null && collision.id != tournamentId) {
            throw ConflictException("A tournament named '$name' already exists.")
        }

        return tournamentRepository.save(
            existing.copy(
                name = name,
                format = format,
                status = status,
                startDate = startDate,
                endDate = endDate,
                capacity = capacity,
                rosterSize = rosterSize,
                creditGrant = creditGrant,
            )
        )
    }

    /**
     * Delete a tournament and all its related data.
     *
     * All foreign keys have `ON DELETE CASCADE`, so this operation will
     * automatically remove:
     * - tournament_heroes entries (hero pool with prices)
     * - tournament_maps entries (legal board pool)
     * - tournament_entries entries (manager registrations)
     * - scoring_rule_sets entries and their coefficients
     * - tournament_matches entries and their participants/bans
     *
     * The operation is allowed for any tournament status, but requires that
     * the tournament exists.
     *
     * The [MatchResultCache] drop is hygiene rather than correctness: every
     * standings route calls [TournamentService.requireTournament] first and so
     * 404s before a cached list could be served. It just stops a deleted
     * tournament's matches occupying the cache until something evicts them.
     */
    @Transactional
    fun delete(tournamentId: Long) {
        val existing = tournamentService.requireTournament(tournamentId)
        tournamentRepository.delete(existing)
        matchResultCache.invalidate(tournamentId)
    }
}
