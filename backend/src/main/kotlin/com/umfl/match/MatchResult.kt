package com.umfl.match

import com.umfl.scoring.HeroRole
import com.umfl.scoring.MetricContext
import java.time.Instant

/**
 * A recorded match, as read back out of the database.
 *
 * Deliberately plain records with no Spring Data annotations: nothing in this
 * application writes a match. Results are facts an admin records (today, via
 * Flyway seed SQL), and everything downstream -- standings, ticker, points --
 * is derived from them at read time.
 */
data class ParticipantResult(
    val participantId: Long,
    /** Who piloted the hero, as free text. Null when the result was recorded unattributed. */
    val playerLabel: String?,
    val heroId: Long,
    val heroName: String,
    /** The hero's health at the end. 0 means defeated. */
    val healthRemaining: Int,
    val isWinner: Boolean,
)

/** A hero banned out of one match. */
data class BanResult(
    val heroId: Long,
    val heroName: String,
)

data class MatchResult(
    val matchId: Long,
    val tournamentId: Long,
    val round: Int,
    val mapId: Long,
    val mapName: String,
    val playedAt: Instant,
    val participants: List<ParticipantResult>,
    val bans: List<BanResult>,
) {
    /**
     * False for a timed draw: nobody is flagged as the winner, yet both sides
     * may still be alive on unequal health.
     */
    val hasWinner: Boolean
        get() = participants.any { it.isWinner }

    fun opponentsOf(heroId: Long): List<ParticipantResult> =
        participants.filter { it.heroId != heroId }

    /**
     * Every hero this match touched -- played or banned -- exactly once, each
     * carrying its role so the metric extractors can price it.
     *
     * A hero cannot both play and be banned in the same match — [MatchResultPolicy]
     * rejects that submission as `DUPLICATE_HERO`, so the de-duplication below is a
     * backstop for data that predates the check, not a supported input. If a record
     * ever says otherwise, playing wins, because it has real health and a real
     * result attached.
     */
    fun heroContexts(): List<MetricContext> {
        val played = participants.map { MetricContext(this, it.heroId, HeroRole.Played(it)) }
        val playedHeroIds = participants.mapTo(mutableSetOf()) { it.heroId }
        val banned = bans
            .filterNot { it.heroId in playedHeroIds }
            .distinctBy { it.heroId }
            .map { MetricContext(this, it.heroId, HeroRole.Banned) }
        return played + banned
    }
}
