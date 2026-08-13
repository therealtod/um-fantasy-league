package com.umfl.api

import com.umfl.hero.HeroView
import com.umfl.manager.Manager
import com.umfl.tournament.BudgetStatus
import com.umfl.tournament.EntryStatus
import com.umfl.tournament.RosterPick
import com.umfl.tournament.RosterPolicy
import com.umfl.tournament.RosterSnapshot
import com.umfl.tournament.Tournament
import com.umfl.tournament.TournamentFormat
import com.umfl.tournament.TournamentStatus
import jakarta.validation.constraints.NotNull
import java.time.Instant
import java.time.LocalDate

data class ManagerDto(
    val id: Long,
    val handle: String,
    val displayName: String,
    val rank: String,
    val isAdmin: Boolean,
) {
    companion object {
        fun from(manager: Manager) = ManagerDto(
            id = requireNotNull(manager.id),
            handle = manager.handle,
            displayName = manager.displayName,
            rank = manager.rankLabel,
            isAdmin = manager.isAdmin,
        )
    }
}

/** A hero and what it costs in the tournament that was asked about. */
data class HeroDto(
    val id: Long,
    val name: String,
    val imageUrl: String?,
    val cost: Int,
) {
    companion object {
        fun from(view: HeroView) = HeroDto(
            id = view.id,
            name = view.name,
            imageUrl = view.imageUrl,
            cost = view.cost,
        )
    }
}

/**
 * Dates serialise as plain `YYYY-MM-DD`: a tournament is announced as a day or
 * a weekend, not a wall-clock instant.
 */
data class TournamentDto(
    val id: Long,
    val name: String,
    val format: TournamentFormat,
    val status: TournamentStatus,
    val startDate: LocalDate,
    val endDate: LocalDate?,
    val capacity: Int,
    val enrolled: Int,
    val rosterSize: Int,
    val creditGrant: Int,
    val acceptsRegistration: Boolean,
    /** Null when the calling manager has not entered this tournament. */
    val myEntryStatus: EntryStatus?,
) {
    companion object {
        fun from(tournament: Tournament, enrolled: Int, myEntryStatus: EntryStatus?) = TournamentDto(
            id = requireNotNull(tournament.id),
            name = tournament.name,
            format = tournament.format,
            status = tournament.status,
            startDate = tournament.startDate,
            endDate = tournament.endDate,
            capacity = tournament.capacity,
            enrolled = enrolled,
            rosterSize = tournament.rosterSize,
            creditGrant = tournament.creditGrant,
            acceptsRegistration = tournament.acceptsRegistration,
            myEntryStatus = myEntryStatus,
        )
    }
}

data class BudgetStatusDto(
    val spent: Int,
    val creditGrant: Int,
    val remaining: Int,
    val utilisation: Double,
) {
    companion object {
        fun from(status: BudgetStatus) = BudgetStatusDto(
            spent = status.spent,
            creditGrant = status.creditGrant,
            remaining = status.remaining,
            utilisation = status.utilisation,
        )
    }
}

data class RosterDto(
    val entryId: Long,
    val tournamentId: Long,
    val tournamentName: String,
    val status: EntryStatus,
    val locked: Boolean,
    val lockedAt: Instant?,
    val rosterSize: Int,
    val heroes: List<HeroDto>,
    val budget: BudgetStatusDto,
    /**
     * True when [RosterPolicy.validateLock] raises no violations for this roster —
     * i.e. the lock button may enable.
     */
    val lockable: Boolean,
) {
    companion object {
        fun from(snapshot: RosterSnapshot): RosterDto {
            val entry = snapshot.entry
            return RosterDto(
                entryId = requireNotNull(entry.id),
                tournamentId = requireNotNull(snapshot.tournament.id),
                tournamentName = snapshot.tournament.name,
                status = entry.status,
                locked = entry.isLocked,
                lockedAt = entry.lockedAt,
                rosterSize = snapshot.tournament.rosterSize,
                heroes = snapshot.heroes.map(HeroDto::from),
                budget = BudgetStatusDto.from(snapshot.budget),
                lockable = RosterPolicy.validateLock(
                    picks = snapshot.heroes.map { RosterPick(it.id, it.cost) },
                    tournament = snapshot.tournament,
                    entry = entry,
                ).isEmpty(),
            )
        }
    }
}

data class SetSlotsRequest(
    @field:NotNull(message = "heroIds is required")
    val heroIds: List<Long>?,
)
