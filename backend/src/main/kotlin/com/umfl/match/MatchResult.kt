package com.umfl.match

import com.umfl.scoring.HeroRole
import com.umfl.scoring.MetricContext
import java.time.Instant

/**
 * A recorded match, as read back out of the database.
 *
 * Deliberately plain records with no Spring Data annotations: reads never go
 * through the [TournamentMatch] write aggregate (see [MatchResultQuery]'s
 * class doc). Everything downstream -- standings, ticker, points -- is
 * derived from these at read time.
 */
data class MatchParticipantResult(
    /** 0 or 1 — a stable ordinal for the whole series, matching `match_participant.side`. */
    val side: Int,
    /** Who piloted this side, as free text. Null when recorded unattributed. */
    val playerLabel: String?,
    /**
     * The heroes this side drafted for the series, whether or not it fielded
     * them. Every hero that played one of the games is in here — a recorded
     * draft is complete ([MatchRule.PLAYED_HERO_NOT_DRAFTED]) — so the ones
     * that appear in no game are the picks that never hit the table.
     */
    val draftedHeroes: List<DraftedHeroResult> = emptyList(),
)

/** One hero on one side's draft board. */
data class DraftedHeroResult(
    val heroId: Long,
    val heroName: String,
)

data class GameParticipantResult(
    val side: Int,
    val heroId: Long,
    val heroName: String,
    /** The hero's health at the end of this game. 0 means defeated. */
    val healthRemaining: Int,
    val isWinner: Boolean,
)

data class GameResult(
    val gameId: Long,
    val gameNumber: Int,
    val mapId: Long,
    val mapName: String,
    val participants: List<GameParticipantResult>,
) {
    /**
     * The side that took this game. Never null for a recorded game:
     * [MatchResultPolicy] rejects a submission whose game has anything other
     * than exactly one winner, so there is no drawn game to represent.
     */
    val winner: GameParticipantResult?
        get() = participants.firstOrNull { it.isWinner }

    fun opponentsOf(heroId: Long): List<GameParticipantResult> =
        participants.filter { it.heroId != heroId }
}

/**
 * A hero banned out of the series, categorized by when/why it was struck.
 *
 * [side] is whose draft it came out of; [banType] is who struck it. Null for a
 * `PRE_BAN` -- struck before sides were assigned, so it belongs to neither --
 * and for anything recorded before `hero_ban.side` existed.
 */
data class BanResult(
    val heroId: Long,
    val heroName: String,
    val banType: BanType,
    val side: Int? = null,
)

data class MatchResult(
    val matchId: Long,
    val tournamentId: Long,
    val round: Int,
    val playedAt: Instant,
    val externalLink: String,
    val participants: List<MatchParticipantResult>,
    /** Ordered by [GameResult.gameNumber]. */
    val games: List<GameResult>,
    val bans: List<BanResult>,
) {
    fun playerLabelForSide(side: Int): String? =
        participants.firstOrNull { it.side == side }?.playerLabel

    /** Every hero either side drafted, once for the series however many sides took it. */
    fun draftedHeroIds(): List<Long> =
        participants.flatMap { it.draftedHeroes }.map { it.heroId }.distinct()

    /**
     * The match's contexts, in the three shapes a hero can have here.
     *
     * `Played` is per game: every (hero, game) this match's games touched, so
     * a hero played in two games of a Bo3 yields two contexts, each scoring
     * independently. `Drafted` and `Banned` are both per *series*, exactly
     * once each, because the draft happens once before any game is played and
     * must not be multiplied by game count -- which is why a hero that plays
     * all three games of a Bo3 still appears once.
     *
     * A drafted hero that played therefore yields both: N `Played` contexts
     * for the per-game metrics, plus the one `Drafted` context APPEARANCE
     * prices.
     *
     * A hero can be none of drafted, played or banned in the same match —
     * [MatchResultPolicy] rejects those submissions as `BANNED_HERO_DRAFTED`
     * and `BANNED_HERO_PLAYED` — so the exclusions below are a backstop for
     * data that predates the checks, not supported input. Playing wins over a
     * ban, because it has real health and a real result attached, and a hero
     * that played is credited with the draft that fielded it.
     */
    fun heroContexts(): List<MetricContext> {
        val played = games.flatMap { game ->
            game.participants.map { participant ->
                MetricContext(this, participant.heroId, HeroRole.Played(game, participant))
            }
        }
        val playedHeroIds = played.mapTo(mutableSetOf()) { it.heroId }
        val banned = bans
            .filterNot { it.heroId in playedHeroIds }
            .distinctBy { it.heroId }
            .map { MetricContext(this, it.heroId, HeroRole.Banned) }
        val bannedHeroIds = banned.mapTo(mutableSetOf()) { it.heroId }
        val drafted = draftedHeroIds()
            .filterNot { it in bannedHeroIds }
            .map { MetricContext(this, it, HeroRole.Drafted) }
        return played + drafted + banned
    }
}
