package com.umfl.matchimport

import com.umfl.match.BanType
import java.time.Instant

/**
 * A scraped match resolved against one tournament's catalogue, ready for an
 * admin to review — not a recorded match.
 *
 * The import path deliberately stops here. Everything this carries still has to
 * go through `POST /api/admin/tournaments/{id}/matches` and therefore through
 * `MatchResultPolicy`, exactly as a hand-typed match does, so importing can
 * never write a result that manual entry would have rejected.
 *
 * Two fields are null by design rather than by failure: [round], because the
 * source names its pools ("The Wayward Sisters") where the schema wants a
 * positive `Int`, and [playedAt] when the source's timestamp carried a timezone
 * abbreviation that couldn't be resolved. Both are for the admin to fill in, and
 * [roundName] / [playedAtRaw] are carried through so they have something to go on.
 */
data class MatchImportPreview(
    val sourceUrl: String,
    /** The source's own name for the round — context for the admin, never stored. */
    val roundName: String?,
    /** e.g. `"BO3"` — context only; nothing in this domain records a series format. */
    val seriesFormat: String?,
    val playedAt: Instant?,
    val playedAtRaw: String?,
    /**
     * Set when this tournament already has a match whose `external_link` is
     * [sourceUrl]. A warning, not a block: correcting an imported match by
     * re-importing it is legitimate, and nothing in the schema makes the link
     * unique.
     */
    val alreadyImportedMatchId: Long?,
    val participants: List<ImportedParticipant>,
    val games: List<ImportedGame>,
    val bans: List<ImportedBan>,
    /** Every name that could not be resolved. Non-empty means the draft is incomplete. */
    val unresolved: List<UnresolvedName>,
)

data class ImportedParticipant(
    val playerLabel: String?,
    /**
     * Every hero this side drafted, fielded or not — the full list, matching
     * `MatchParticipantRequest.draftedHeroIds`. Note the admin *form* holds only
     * the unfielded subset and re-unions at save time, so the frontend subtracts
     * before seeding the wizard.
     */
    val draftedHeroIds: List<Long>,
)

data class ImportedGame(
    val gameNumber: Int,
    val mapId: Long?,
    val mapName: String?,
    val participants: List<ImportedGameParticipant>,
)

data class ImportedGameParticipant(
    val heroId: Long?,
    val heroName: String?,
    val healthRemaining: Int,
    val isWinner: Boolean,
)

data class ImportedBan(
    val heroId: Long?,
    val heroName: String?,
    val banType: BanType,
)

/** What kind of row a name failed to resolve to. */
enum class UnresolvedKind { HERO, MAP }

/**
 * Why a name could not be used.
 *
 * [MAP_NOT_IN_POOL] is the one that will actually fire in practice: `match_game`
 * carries a composite foreign key onto `tournament_map`, so a board this league
 * knows about but hasn't added to *this tournament's* pool cannot be recorded
 * against it. Heroes have no such constraint — `match_game_participant.hero_id`
 * and `hero_ban.hero_id` reference `heroes(id)` directly, never `tournament_hero`.
 */
enum class UnresolvedReason { UNKNOWN_HERO, UNKNOWN_MAP, MAP_NOT_IN_POOL }

data class UnresolvedName(
    val kind: UnresolvedKind,
    /** The name exactly as the source site rendered it, so the admin can recognise it. */
    val sourceName: String,
    val reason: UnresolvedReason,
    /** Set for [UnresolvedReason.MAP_NOT_IN_POOL] — the board exists, it just isn't in the pool. */
    val mapId: Long?,
    val message: String,
)
