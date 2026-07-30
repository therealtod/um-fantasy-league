package com.umfl.tournament

import org.springframework.data.annotation.Id
import org.springframework.data.relational.core.mapping.MappedCollection
import org.springframework.data.relational.core.mapping.Table
import java.time.Instant

enum class EntryStatus {
    /** Roster is still being drafted and may be changed. */
    DRAFT,

    /** Roster is committed and immutable. */
    LOCKED,
}

/**
 * One manager's entry into one tournament — the aggregate root that owns the
 * roster slots.
 *
 * [creditGrant] is copied off the tournament at registration, which is what
 * makes a granted budget stable for the manager who received it.
 *
 * The roster's cost is deliberately *not* stored anywhere: it is the live sum of
 * the slots' `tournament_hero.cost`, so the two can never disagree, and an
 * unlocked roster simply re-prices when an admin retunes the pool.
 */
@Table("tournament_entry")
data class TournamentEntry(
    @Id val id: Long? = null,
    val tournamentId: Long,
    val managerId: Long,
    val status: EntryStatus = EntryStatus.DRAFT,
    val creditGrant: Int,
    val registeredAt: Instant = Instant.now(),
    val lockedAt: Instant? = null,
    @MappedCollection(idColumn = "entry_id", keyColumn = "slot_index")
    val slots: List<EntrySlot> = emptyList(),
) {
    val heroIds: List<Long>
        get() = slots.map { it.heroId }

    val isLocked: Boolean
        get() = status == EntryStatus.LOCKED

    fun withSlots(newSlots: List<EntrySlot>): TournamentEntry = copy(slots = newSlots)

    fun lock(at: Instant = Instant.now()): TournamentEntry =
        copy(status = EntryStatus.LOCKED, lockedAt = at)
}

/**
 * A single hero on a roster. Child of the [TournamentEntry] aggregate; its
 * position is the list index, persisted to `entry_slot.slot_index`.
 */
@Table("entry_slot")
data class EntrySlot(
    val heroId: Long,
)
