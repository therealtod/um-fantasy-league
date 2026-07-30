package com.umfl.match

import org.springframework.data.annotation.Id
import org.springframework.data.relational.core.mapping.MappedCollection
import org.springframework.data.relational.core.mapping.Table
import java.time.Instant

/**
 * A recorded match — the aggregate root the admin API writes. Read access
 * stays on [MatchResultQuery]'s hand-written projection (see its class doc);
 * this class exists only so an admin write can save participants and bans
 * together, exactly like [com.umfl.tournament.TournamentEntry] saves its
 * slots as one unit.
 *
 * Children are mapped as [Set], not [List]: unlike `entry_slot.slot_index`,
 * neither `match_participant` nor `match_ban` has a "list position" column —
 * their non-id columns are all real data — so there is nothing for a
 * `keyColumn` to populate and no meaningful order between two sides of a
 * match.
 */
@Table("tournament_match")
data class TournamentMatch(
    @Id val id: Long? = null,
    val tournamentId: Long,
    val round: Int,
    val mapId: Long,
    val playedAt: Instant,
    @MappedCollection(idColumn = "match_id")
    val participants: Set<MatchParticipant> = emptySet(),
    @MappedCollection(idColumn = "match_id")
    val bans: Set<MatchBan> = emptySet(),
)

@Table("match_participant")
data class MatchParticipant(
    @Id val id: Long? = null,
    /** Free text, no `player` table behind it — see the column comment in `V1__core_schema.sql`. */
    val playerLabel: String? = null,
    val heroId: Long,
    val healthRemaining: Int,
    val isWinner: Boolean = false,
)

/** No surrogate id: `(match_id, hero_id)` is the natural composite key. */
@Table("match_ban")
data class MatchBan(val heroId: Long)
