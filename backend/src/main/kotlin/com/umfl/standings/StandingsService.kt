package com.umfl.standings

import com.umfl.match.MatchResultQuery
import com.umfl.scoring.MatchMetrics
import com.umfl.scoring.ScoringEngine
import com.umfl.scoring.ScoringRuleSetQuery
import com.umfl.scoring.ScoringRules
import org.slf4j.LoggerFactory
import org.springframework.stereotype.Service
import java.time.Instant

/**
 * One leaderboard column.
 *
 * The board carries its own column definitions because the backend does not
 * know which columns exist until it has read `scoring_coefficient` — an admin
 * adds a metric with an INSERT, not a redeploy.
 */
data class MetricColumn(
    val metric: String,
    val label: String,
    val coefficient: Double,
)

data class StandingsRow(
    val rank: Int,
    val entryId: Long,
    val managerId: Long,
    val handle: String,
    val displayName: String,
    val roster: List<String>,
    val spent: Int,
    val creditGrant: Int,
    val totalPoints: Double,
    /** Points earned in the tournament's latest round — the "LAST RD" column. */
    val roundPoints: Double,
    /** Keyed by [MetricColumn.metric]. Unknown metrics never appear. */
    val breakdown: Map<String, Double>,
)

data class StandingsBoard(
    val tournamentId: Long,
    val ruleSetName: String,
    val currentRound: Int,
    val metrics: List<MetricColumn>,
    val rows: List<StandingsRow>,
)

data class TickerSide(
    /** Free text, absent from the JSON when the result was recorded unattributed. */
    val playerLabel: String?,
    val heroName: String,
    val healthRemaining: Int,
    val isWinner: Boolean,
    /** This hero's net score for this match. May be negative. */
    val points: Double,
)

data class TickerEntry(
    val matchId: Long,
    val round: Int,
    val mapName: String,
    val playedAt: Instant,
    /** Winner first. A timed draw has no winner at all. */
    val sides: List<TickerSide>,
    val bannedHeroNames: List<String>,
)

/**
 * Folds recorded matches into a leaderboard.
 *
 * Points are computed here at read time and never stored. Coefficients are
 * mutable reference data, so a stored total would be a cache with nothing to
 * invalidate it; at tournament scale the fold is microseconds, and each
 * (hero, match) pair is priced exactly once regardless of how many rosters hold
 * that hero.
 */
@Service
class StandingsService(
    private val standingsQuery: StandingsQuery,
    private val matchResultQuery: MatchResultQuery,
    private val scoringRuleSetQuery: ScoringRuleSetQuery,
) {

    private val log = LoggerFactory.getLogger(javaClass)

    fun board(tournamentId: Long): StandingsBoard {
        val rules = resolveRules(tournamentId)
        val matches = matchResultQuery.findByTournament(tournamentId)
        val currentRound = matches.maxOfOrNull { it.round } ?: 0

        // Every hero the tournament touched, priced once, keyed for roster lookup.
        val appearancesByHero = matches
            .flatMap { match ->
                match.heroContexts().map { context ->
                    ScoredAppearance(
                        heroId = context.heroId,
                        round = match.round,
                        breakdown = ScoringEngine.breakdown(context, rules),
                    )
                }
            }
            .groupBy { it.heroId }

        val unranked = standingsQuery.rosters(tournamentId).map { entry ->
            // Dense: every scored metric gets a column value, even a zero one.
            val totals = rules.scoredMetrics.associateWithTo(LinkedHashMap()) { 0.0 }
            var roundPoints = 0.0

            for (hero in entry.heroes) {
                for (appearance in appearancesByHero[hero.heroId].orEmpty()) {
                    for ((metric, points) in appearance.breakdown) {
                        totals[metric] = (totals[metric] ?: 0.0) + points
                        if (appearance.round == currentRound) roundPoints += points
                    }
                }
            }

            val breakdown = totals.mapValues { (_, points) -> ScoringEngine.round2(points) }
            StandingsRow(
                rank = 0, // replaced once the board is ordered
                entryId = entry.entryId,
                managerId = entry.managerId,
                handle = entry.handle,
                displayName = entry.displayName,
                roster = entry.heroes.map { it.name },
                spent = entry.spent,
                creditGrant = entry.creditGrant,
                totalPoints = ScoringEngine.round2(breakdown.values.sum()),
                roundPoints = ScoringEngine.round2(roundPoints),
                breakdown = breakdown,
            )
        }

        return StandingsBoard(
            tournamentId = tournamentId,
            ruleSetName = rules.name,
            currentRound = currentRound,
            metrics = rules.scoredMetrics.map { metric ->
                MetricColumn(
                    metric = metric,
                    label = MatchMetrics.label(metric),
                    coefficient = rules.coefficientOf(metric).toDouble(),
                )
            },
            rows = rank(unranked),
        )
    }

    /**
     * The newest recorded matches, as the Standings ticker renders them.
     *
     * [sinceMatchId] is the polling key rather than a timestamp: parallel tables
     * share a `played_at`, while the match id is a monotonic bigserial.
     */
    fun ticker(tournamentId: Long, sinceMatchId: Long = 0, limit: Int = 25): List<TickerEntry> {
        val rules = resolveRules(tournamentId)
        return matchResultQuery.findByTournamentSince(tournamentId, sinceMatchId, limit).map { match ->
            val contextsByHero = match.heroContexts().associateBy { it.heroId }
            TickerEntry(
                matchId = match.matchId,
                round = match.round,
                mapName = match.mapName,
                playedAt = match.playedAt,
                // Stable sort: the winner floats up, the rest keep recorded order.
                sides = match.participants.sortedByDescending { it.isWinner }.map { participant ->
                    TickerSide(
                        playerLabel = participant.playerLabel,
                        heroName = participant.heroName,
                        healthRemaining = participant.healthRemaining,
                        isWinner = participant.isWinner,
                        points = contextsByHero[participant.heroId]
                            ?.let { ScoringEngine.score(it, rules) }
                            ?: 0.0,
                    )
                },
                bannedHeroNames = match.bans.map { it.heroName },
            )
        }
    }

    private fun resolveRules(tournamentId: Long): ScoringRules =
        scoringRuleSetQuery.activeRules(tournamentId).also { rules ->
            if (rules.unknownMetrics.isNotEmpty()) {
                log.info(
                    "Tournament {} weights metric(s) {} that no extractor implements; they score zero.",
                    tournamentId,
                    rules.unknownMetrics,
                )
            }
        }

    /**
     * Standard competition ranking (1, 2, 2, 4). Ties are ordinary on a finished
     * tournament — two managers who drafted overlapping rosters can genuinely
     * land on the same total — so positional `index + 1` would lie.
     */
    private fun rank(rows: List<StandingsRow>): List<StandingsRow> {
        val ordered = rows.sortedWith(
            compareByDescending<StandingsRow> { it.totalPoints }.thenBy { it.handle }
        )
        var currentRank = 0
        var previousPoints: Double? = null
        return ordered.mapIndexed { index, row ->
            if (previousPoints == null || row.totalPoints != previousPoints) {
                currentRank = index + 1
                previousPoints = row.totalPoints
            }
            row.copy(rank = currentRank)
        }
    }

    private data class ScoredAppearance(
        val heroId: Long,
        val round: Int,
        val breakdown: Map<String, Double>,
    )
}
