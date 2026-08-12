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
     * The side that took this game, if any. Null for a timed draw: nobody is
     * flagged as the winner, yet both sides may still be alive.
     */
    val winner: GameParticipantResult?
        get() = participants.firstOrNull { it.isWinner }

    fun opponentsOf(heroId: Long): List<GameParticipantResult> =
        participants.filter { it.heroId != heroId }
}

/** A hero banned out of the series, categorized by when/why it was struck. */
data class BanResult(
    val heroId: Long,
    val heroName: String,
    val banType: BanType,
)

data class MatchResult(
    val matchId: Long,
    val tournamentId: Long,
    val round: Int,
    val playedAt: Instant,
    val externalLink: String?,
    val participants: List<MatchParticipantResult>,
    /** Ordered by [GameResult.gameNumber]. */
    val games: List<GameResult>,
    val bans: List<BanResult>,
) {
    fun playerLabelForSide(side: Int): String? =
        participants.firstOrNull { it.side == side }?.playerLabel

    /**
     * Every (hero, game) this match's games touched, once per game played --
     * a hero played in two games of a Bo3 yields two `Played` contexts, each
     * scoring independently -- plus every banned hero exactly once for the
     * whole series, regardless of how many games it has: a ban is struck once,
     * before any game is played, so it must not be multiplied by game count.
     *
     * A hero cannot both play (in any game) and be banned in the same match —
     * [MatchResultPolicy] rejects that submission as `BANNED_HERO_PLAYED`, so
     * the de-duplication below is a backstop for data that predates the check,
     * not a supported input. If a record ever says otherwise, playing wins,
     * because it has real health and a real result attached.
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
        return played + banned
    }
}
