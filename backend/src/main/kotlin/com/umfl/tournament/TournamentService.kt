package com.umfl.tournament

import com.umfl.common.ConflictException
import com.umfl.common.NotFoundException
import com.umfl.common.RosterRuleException
import com.umfl.hero.HeroQueryRepository
import com.umfl.hero.HeroView
import com.umfl.manager.Manager
import org.springframework.stereotype.Service
import org.springframework.transaction.annotation.Transactional
import java.time.Instant

/** An entry together with the heroes on it, priced now, and its budget position. */
data class RosterSnapshot(
    val entry: TournamentEntry,
    val tournament: Tournament,
    val heroes: List<HeroView>,
    val budget: BudgetStatus,
)

@Service
class TournamentService(
    private val tournamentRepository: TournamentRepository,
    private val entryRepository: TournamentEntryRepository,
    private val heroQueryRepository: HeroQueryRepository,
) {

    fun listTournaments(status: TournamentStatus?): List<Tournament> =
        if (status == null) {
            tournamentRepository.findAllByOrderByStartDateAsc()
        } else {
            tournamentRepository.findByStatusOrderByStartDateAsc(status)
        }

    fun enrolmentCounts(): Map<Long, Int> =
        entryRepository.countEntriesPerTournament().associate { it.tournamentId to it.entryCount }

    fun enrolmentCount(tournamentId: Long): Int = entryRepository.countByTournamentId(tournamentId)

    fun requireTournament(id: Long): Tournament =
        tournamentRepository.findById(id).orElseThrow { NotFoundException("No tournament with id $id") }

    /**
     * Enter a manager into a tournament and open an empty DRAFT roster.
     *
     * Entering is free: there is no fee and no wallet. What registration does is
     * hand over a budget — the tournament's `credit_grant`, snapshotted onto the
     * entry so a later retune cannot disturb a roster drafted against it.
     *
     * The two ways this can race are guarded differently. Double registration is
     * caught by `unique (tournament_id, manager_id)` even if the read below misses
     * a concurrent insert. Capacity has no such constraint — a count is not a
     * row — so the last seat is protected by locking the tournament row first:
     * concurrent registrations for the same tournament serialise there, and each
     * one counts entries only after the previous has committed its own.
     */
    @Transactional
    fun register(tournamentId: Long, manager: Manager): RosterSnapshot {
        val tournament = requireTournament(tournamentId)
        val managerId = requireNotNull(manager.id) { "Manager must be persisted" }

        if (!tournament.acceptsRegistration) {
            throw ConflictException("${tournament.name} is ${tournament.status} and is not open for registration.")
        }
        if (entryRepository.findByTournamentIdAndManagerId(tournamentId, managerId) != null) {
            throw ConflictException("Already registered for ${tournament.name}.")
        }

        tournamentRepository.lockById(tournamentId)
            ?: throw NotFoundException("No tournament with id $tournamentId")
        if (entryRepository.countByTournamentId(tournamentId) >= tournament.capacity) {
            throw ConflictException("${tournament.name} is full (${tournament.capacity} entries).")
        }

        val entry = entryRepository.save(
            TournamentEntry(
                tournamentId = tournamentId,
                managerId = managerId,
                status = EntryStatus.DRAFT,
                creditGrant = tournament.creditGrant,
                registeredAt = Instant.now(),
            )
        )
        return snapshot(entry, tournament)
    }

    fun findMyEntry(tournamentId: Long, manager: Manager): RosterSnapshot? {
        val tournament = requireTournament(tournamentId)
        val managerId = requireNotNull(manager.id) { "Manager must be persisted" }
        val entry = entryRepository.findByTournamentIdAndManagerId(tournamentId, managerId) ?: return null
        return snapshot(entry, tournament)
    }

    private fun requireMyEntry(tournamentId: Long, manager: Manager): TournamentEntry {
        val managerId = requireNotNull(manager.id) { "Manager must be persisted" }
        return entryRepository.findByTournamentIdAndManagerId(tournamentId, managerId)
            ?: throw NotFoundException("You are not registered for tournament $tournamentId.")
    }

    /**
     * Replace the roster selection.
     *
     * Over-budget selections are accepted while the entry is a DRAFT — the
     * builder is a scratchpad and shows a negative meter rather than blocking
     * the edit — and rejected at [lockRoster].
     */
    @Transactional
    fun setSlots(tournamentId: Long, manager: Manager, heroIds: List<Long>): RosterSnapshot {
        val tournament = requireTournament(tournamentId)
        val entry = requireMyEntry(tournamentId, manager)

        val picks = resolvePicks(tournament, heroIds)
        val violations = RosterPolicy.validateDraft(picks, tournament, entry.status)
        if (violations.isNotEmpty()) throw RosterRuleException(violations)

        // Only the hero is persisted: no cost snapshot, so an unlocked roster
        // re-prices when an admin retunes the pool.
        val updated = entryRepository.save(entry.withSlots(picks.map { EntrySlot(heroId = it.heroId) }))
        return snapshot(updated, tournament)
    }

    /** Commit the roster. Enforces roster size and the entry's credit grant. */
    @Transactional
    fun lockRoster(tournamentId: Long, manager: Manager): RosterSnapshot {
        val tournament = requireTournament(tournamentId)
        val entry = requireMyEntry(tournamentId, manager)

        val picks = resolvePicks(tournament, entry.heroIds)
        val violations = RosterPolicy.validateLock(picks, tournament, entry)
        if (violations.isNotEmpty()) throw RosterRuleException(violations)

        val locked = entryRepository.save(entry.lock())
        return snapshot(locked, tournament)
    }

    /**
     * Turn requested hero ids into priced picks, preserving the caller's ordering
     * so slot positions are stable.
     *
     * The price comes from `tournament_hero`, so "unknown" means *not in this
     * tournament's pool* — a stronger and more correct check than asking whether
     * the hero exists at all.
     */
    private fun resolvePicks(tournament: Tournament, heroIds: List<Long>): List<RosterPick> {
        if (heroIds.isEmpty()) return emptyList()

        val tournamentId = requireNotNull(tournament.id) { "Tournament must be persisted" }
        val byId = heroQueryRepository.findByIds(tournamentId, heroIds.toSet()).associateBy { it.id }

        val unknown = heroIds.filterNot(byId::containsKey)
        if (unknown.isNotEmpty()) {
            throw RosterRuleException(
                listOf(
                    RosterViolation(
                        RosterRule.UNKNOWN_HERO,
                        "${tournament.name} has no hero for id(s): ${unknown.distinct().sorted().joinToString()}.",
                    )
                )
            )
        }
        // Duplicates survive this mapping on purpose — RosterPolicy reports them.
        return heroIds.map { RosterPick(heroId = it, cost = byId.getValue(it).cost) }
    }

    private fun snapshot(entry: TournamentEntry, tournament: Tournament): RosterSnapshot {
        val tournamentId = requireNotNull(tournament.id) { "Tournament must be persisted" }
        // findRosterHeroes, not findByIds: a slot already committed to the
        // roster must still be reported (at cost 0) even if the hero has since
        // left this tournament's pool — see UMFL-06.
        val heroesById = heroQueryRepository
            .findRosterHeroes(tournamentId, entry.heroIds.toSet())
            .associateBy { it.id }
        // Ordered by slot, not by the query's ordering.
        val heroes = entry.heroIds.mapNotNull(heroesById::get)
        return RosterSnapshot(
            entry = entry,
            tournament = tournament,
            heroes = heroes,
            budget = RosterPolicy.budgetStatus(
                heroes.map { RosterPick(it.id, it.cost) },
                entry.creditGrant,
            ),
        )
    }
}
