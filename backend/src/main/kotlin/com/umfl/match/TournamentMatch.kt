package com.umfl.match

import org.springframework.data.annotation.Id
import org.springframework.data.relational.core.mapping.MappedCollection
import org.springframework.data.relational.core.mapping.Table
import java.time.Instant

/**
 * A recorded match — the aggregate root the admin API writes. Read access
 * stays on [MatchResultQuery]'s hand-written projection (see its class doc);
 * this class exists only so an admin write can save participants, games and
 * the draft (picks and bans) together, exactly like
 * [com.umfl.tournament.TournamentEntry] saves its slots as one unit.
 *
 * A match is a series of one or more [games][MatchGame] between the same two
 * [participants][MatchParticipant] — [participants] carries only the two
 * humans, fixed for the whole series; each game records its own hero, board,
 * health and winner, because a side can pilot a different hero per game.
 */
@Table("tournament_matches")
data class TournamentMatch(
    @Id val id: Long? = null,
    val tournamentId: Long,
    val round: Int,
    val playedAt: Instant,
    /**
     * Required, and unique within the tournament — `uq_tournament_match_external_link`
     * is what stops the same match being imported twice. A match with no page
     * anywhere carries a synthetic `urn:umfl:match:<id>` placeholder instead.
     */
    val externalLink: String,
    /**
     * `side` (0 or 1) is the list position, persisted to `match_participants.side` —
     * the same `keyColumn` idiom [com.umfl.tournament.EntrySlot] uses for
     * `slot_index`, so this class carries no explicit `side` field.
     */
    @MappedCollection(idColumn = "match_id", keyColumn = "side")
    val participants: List<MatchParticipant> = emptyList(),
    /**
     * Mapped as a [Set], not a [List]: unlike `side` above, `game_number` is
     * real admin-meaningful data (checked `> 0`), not a pure list-position
     * ordinal, so it stays an explicit field rather than a `keyColumn`.
     */
    @MappedCollection(idColumn = "match_id")
    val games: Set<MatchGame> = emptySet(),
    @MappedCollection(idColumn = "match_id")
    val bans: Set<HeroBan> = emptySet(),
    /**
     * The picks half of the draft, to [bans]' bans half. A [Set] for the same
     * reason: `side` is real data (a pick belongs to one side, and one side
     * owns several picks), not a list-position ordinal.
     *
     * Hangs off the root rather than off [MatchParticipant], where it would
     * read more naturally: `match_participants` has a composite key, and Spring
     * Data JDBC cannot map a child of an entity keyed that way.
     */
    @MappedCollection(idColumn = "match_id")
    val picks: Set<HeroPick> = emptySet(),
)

/** One side of the series — which human played it, for the whole match. */
@Table("match_participants")
data class MatchParticipant(val playerLabel: String? = null)

/**
 * One game within a series.
 *
 * [tournamentId] is denormalized off the owning [TournamentMatch] purely so
 * this table can carry the same composite "map is in this tournament's pool"
 * foreign key `tournament_matches` used to carry directly — nothing besides
 * construction reads it.
 */
@Table("match_games")
data class MatchGame(
    @Id val id: Long? = null,
    val tournamentId: Long,
    val gameNumber: Int,
    val mapId: Long,
    /** See [TournamentMatch.participants] — same `side`-as-list-position idiom. */
    @MappedCollection(idColumn = "game_id", keyColumn = "side")
    val participants: List<MatchGameParticipant> = emptyList(),
)

/** One side's result in one game: the hero it brought and how the game ended. */
@Table("match_game_participants")
data class MatchGameParticipant(
    val heroId: Long,
    val healthRemaining: Int,
    val isWinner: Boolean = false,
)

enum class BanType { PRE_BAN, OPPONENT_BAN, SELF_BAN }

/**
 * No surrogate id: `(match_id, hero_id)` is the natural composite key -- a
 * hero is struck at most once per series however many sides wanted it.
 *
 * [side] is the draft this hero came out of, while [banType] says who struck
 * it. Null for a `PRE_BAN`, which precedes side assignment, and for a ban
 * typed with no side to attribute -- see `hero_bans.side` in
 * `V1__core_schema.sql`.
 */
@Table("hero_bans")
data class HeroBan(val heroId: Long, val banType: BanType, val side: Int? = null)

/**
 * A hero one side drafted for the series, played or not.
 *
 * No surrogate id either: `(match_id, side, hero_id)` is the natural key. No
 * ban category to pair with [HeroBan.banType] -- a pick is a pick.
 */
@Table("match_hero_picks")
data class HeroPick(val side: Int, val heroId: Long)
