package com.umfl.standings

import com.umfl.match.MatchResultCache
import com.umfl.scoring.HeroRole
import com.umfl.scoring.MatchMetrics
import com.umfl.scoring.ScoringEngine
import com.umfl.scoring.ScoringRuleSetQuery
import com.umfl.scoring.ScoringRules
import org.slf4j.LoggerFactory
import org.springframework.stereotype.Service
import org.springframework.transaction.annotation.Isolation
import org.springframework.transaction.annotation.Transactional
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

data class TickerGameSide(
    /** Free text, absent from the JSON when the result was recorded unattributed. */
    val playerLabel: String?,
    val heroName: String,
    val healthRemaining: Int,
    val isWinner: Boolean,
    /** This hero's net score for this game. May be negative. */
    val points: Double,
)

data class TickerGame(
    val gameNumber: Int,
    val mapName: String,
    /** Winner first — every game has one, so this is always winner then loser. */
    val sides: List<TickerGameSide>,
)

data class TickerEntry(
    val matchId: Long,
    val round: Int,
    val playedAt: Instant,
    val externalLink: String,
    /** Ordered by game number — one entry per game played in the series. */
    val games: List<TickerGame>,
    val bannedHeroNames: List<String>,
    /**
     * Heroes drafted for this series that never played a game. They appear in
     * no [games] row, so without naming them here their appearance points come
     * from nowhere the reader can see.
     */
    val draftedUnplayedHeroNames: List<String>,
)

/**
 * Folds recorded matches into a leaderboard.
 *
 * Points are computed here at read time and never stored. Coefficients are
 * mutable reference data, so a stored *total* would be a cache with nothing to
 * invalidate it; at tournament scale the fold is microseconds, and each
 * (hero, match) pair is priced exactly once regardless of how many rosters hold
 * that hero.
 *
 * What is cached is the fold's *input*, in [MatchResultCache] — the assembled
 * match list, which unlike a total has exactly one writer and therefore a
 * complete invalidation signal. The rules, rosters and hero costs read below
 * are deliberately not cached for precisely the reason the paragraph above
 * gives, so re-pricing a hero, retuning a coefficient or changing a roster is
 * visible on the very next request.
 */
@Service
class StandingsService(
    private val standingsQuery: StandingsQuery,
    private val matchResultCache: MatchResultCache,
    private val scoringRuleSetQuery: ScoringRuleSetQuery,
) {

    private val log = LoggerFactory.getLogger(javaClass)

    /**
     * REPEATABLE_READ, not just readOnly: Postgres's default READ COMMITTED
     * gives every *statement* in a transaction its own snapshot, so a plain
     * `@Transactional` still lets a concurrent `AdminMatchService.record` /
     * `correct` / `delete` land between this method's several statements (the
     * rules read, the match fold, [MatchResultQuery.assemble]'s five queries,
     * the rosters read) and skew them against each other — e.g. a delete
     * committing between the match header query and its participants query
     * yields a header whose games/participants come back empty. REPEATABLE_READ
     * pins one snapshot for the whole transaction. It's safe to use here with
     * no retry handling because the transaction is read-only: Postgres only
     * raises a serialization failure on a write/write conflict, which a
     * read-only transaction can never have.
     *
     * [MatchResultCache] narrows what that buys without making it redundant. On
     * a miss the six assembly queries still run inside *this* transaction —
     * the skew above, unchanged. On a hit the list was assembled inside some
     * earlier reader's `REPEATABLE_READ` transaction and is internally coherent
     * in exactly the same way, just against an older snapshot. The guarantee is
     * therefore "the match list is a coherent snapshot", not "every fact on this
     * board came from one snapshot" — the rules read and the roster read are one
     * statement each and describe facts uncorrelated with match writes, so
     * nothing skews against anything. The worst case is a board pricing a match
     * list one write behind, which is the staleness the cache is for and which
     * the SSE push already corrects.
     */
    @Transactional(readOnly = true, isolation = Isolation.REPEATABLE_READ)
    fun board(tournamentId: Long): StandingsBoard {
        val rules = resolveRules(tournamentId)
        val matches = matchResultCache.findByTournament(tournamentId)
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
    /** See the isolation note on [board] — the same cross-statement race applies here. */
    @Transactional(readOnly = true, isolation = Isolation.REPEATABLE_READ)
    fun ticker(tournamentId: Long, sinceMatchId: Long = 0, limit: Int = 25): List<TickerEntry> {
        val rules = resolveRules(tournamentId)
        return matchResultCache.findByTournamentSince(tournamentId, sinceMatchId, limit).map { match ->
            // Built once and partitioned by role, rather than re-filtered from a fresh
            // heroContexts() call per role — same list, two views onto it.
            val contexts = match.heroContexts()

            // Keyed by (game, hero), not hero alone: the same hero can appear in two
            // different games of a series with two different scores.
            val contextsByGameAndHero = contexts
                .filter { it.role is HeroRole.Played }
                .associateBy { context -> (context.role as HeroRole.Played).game.gameId to context.heroId }

            // A `Drafted` context is a match-level fact with no game of its own,
            // but the ticker only has game rows to show points in. Bank it against
            // the hero's first game so the rows still sum to what the match banked
            // on the board; in a Bo3 that makes game 1 worth one appearance more
            // than games 2 and 3, which is the draft being priced once, not drift.
            val firstGameIdByHero = buildMap {
                match.games.sortedBy { it.gameNumber }.forEach { game ->
                    game.participants.forEach { participant -> putIfAbsent(participant.heroId, game.gameId) }
                }
            }
            val draftContextsByGameAndHero = contexts
                .filter { it.role is HeroRole.Drafted }
                .mapNotNull { context ->
                    firstGameIdByHero[context.heroId]?.let { gameId -> (gameId to context.heroId) to context }
                }
                .toMap()

            val playedHeroIds = match.games.flatMapTo(mutableSetOf()) { game ->
                game.participants.map { it.heroId }
            }
            TickerEntry(
                matchId = match.matchId,
                round = match.round,
                playedAt = match.playedAt,
                externalLink = match.externalLink,
                games = match.games.map { game ->
                    TickerGame(
                        gameNumber = game.gameNumber,
                        mapName = game.mapName,
                        // Stable sort: the winner floats up, the rest keep recorded order.
                        sides = game.participants.sortedByDescending { it.isWinner }.map { participant ->
                            TickerGameSide(
                                playerLabel = match.playerLabelForSide(participant.side),
                                heroName = participant.heroName,
                                healthRemaining = participant.healthRemaining,
                                isWinner = participant.isWinner,
                                points = ScoringEngine.round2(
                                    listOfNotNull(
                                        contextsByGameAndHero[game.gameId to participant.heroId],
                                        draftContextsByGameAndHero[game.gameId to participant.heroId],
                                    ).sumOf { ScoringEngine.score(it, rules) }
                                ),
                            )
                        },
                    )
                },
                bannedHeroNames = match.bans.map { it.heroName },
                draftedUnplayedHeroNames = match.participants
                    .flatMap { it.draftedHeroes }
                    .filterNot { it.heroId in playedHeroIds }
                    .distinctBy { it.heroId }
                    .map { it.heroName },
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
