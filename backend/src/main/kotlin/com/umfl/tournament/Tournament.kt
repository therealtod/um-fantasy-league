package com.umfl.tournament

import org.springframework.data.annotation.Id
import org.springframework.data.relational.core.mapping.Table
import java.time.LocalDate

enum class TournamentFormat {
    /** Banquest: a fixed hero pool, drafted ahead of the event. */
    BANQUEST,

    /** Arsenal: heroes are picked and banned match by match. */
    ARSENAL,
}

enum class TournamentStatus {
    /** Announced, but registration has not opened yet. */
    SCHEDULED,

    /** Managers may register and draft. */
    REGISTRATION_OPEN,

    /** Matches are being played; rosters are frozen. */
    LIVE,

    /** Finished; results are final. */
    COMPLETED,
}

/**
 * A real Unmatched tournament, and the fantasy parameters layered over it.
 *
 * Dates are [LocalDate], not `Instant`: a tournament is announced as a day or a
 * weekend, not a wall-clock instant. [endDate] is null until it is over.
 *
 * [creditGrant] is the budget every registrant receives. It is snapshotted onto
 * the entry at registration, so retuning it later cannot disturb rosters that
 * were already drafted against the old number.
 */
@Table("tournament")
data class Tournament(
    @Id val id: Long? = null,
    val name: String,
    val format: TournamentFormat,
    val status: TournamentStatus,
    val startDate: LocalDate,
    val endDate: LocalDate? = null,
    val capacity: Int,
    val rosterSize: Int,
    val creditGrant: Int,
) {
    /** Only an open tournament takes new entries. */
    val acceptsRegistration: Boolean
        get() = status == TournamentStatus.REGISTRATION_OPEN

    /** Rosters freeze once the tournament goes live. */
    val acceptsRosterChanges: Boolean
        get() = status == TournamentStatus.SCHEDULED || status == TournamentStatus.REGISTRATION_OPEN
}
