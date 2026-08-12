package com.umfl.scoring

import com.umfl.match.BanType
import com.umfl.match.GameParticipantResult
import com.umfl.match.GameResult
import com.umfl.match.MatchResult

/**
 * What a hero did in one match: it either played one of the series' games
 * (and there is a game + participant row) or it was banned out of the whole
 * series (and there is not).
 */
sealed interface HeroRole {
    data class Played(val game: GameResult, val participant: GameParticipantResult) : HeroRole

    data object Banned : HeroRole
}

/**
 * One hero's role in one match, with the whole match still visible.
 *
 * The extractors need more than a participant row: `HEALTH_DIFFERENTIAL` needs
 * the opponent, and `SELF_BAN`/`OPPONENT_BAN` have no participant row at all --
 * they read `match.bans` instead. WIN and LOSS are scoped per game rather than
 * per series — a hero that takes game 1 and drops game 2 scores one of each.
 */
data class MetricContext(
    val match: MatchResult,
    val heroId: Long,
    val role: HeroRole,
) {
    val participant: GameParticipantResult?
        get() = (role as? HeroRole.Played)?.participant

    val opponents: List<GameParticipantResult>
        get() = (role as? HeroRole.Played)?.game?.opponentsOf(heroId).orEmpty()
}

/**
 * The registry of scoring metrics this application knows how to measure.
 *
 * `scoring_coefficient.metric` is free-form text so an admin can add a weighted
 * row without a migration. This registry prices the keys it implements and
 * silently ignores the rest: an unknown metric contributes nothing, is dropped
 * from the leaderboard's columns, and throws nothing.
 *
 * There is deliberately no `DAMAGE_DEALT`: `heroes` carries no starting-health
 * column, so damage is not derivable from `health_remaining`. Adding it is a
 * schema decision, not a registry one.
 */
object MatchMetrics {

    private val extractors: Map<String, (MetricContext) -> Double> = linkedMapOf(
        "APPEARANCE" to ::appearance,
        "SELF_BAN" to ::selfBan,
        "OPPONENT_BAN" to ::opponentBan,
        "WIN" to ::win,
        "LOSS" to ::loss,
        "HEALTH_REMAINING" to ::healthRemaining,
        "HEALTH_DIFFERENTIAL" to ::healthDifferential,
        "SHUTOUT" to ::shutout,
    )

    /** Every metric key this build implements, in a stable order. */
    val known: Set<String>
        get() = extractors.keys

    /** The extractor for [metric], or null when nothing implements it. */
    operator fun get(metric: String): ((MetricContext) -> Double)? = extractors[normalise(metric)]

    /** Trim and upper-case, so `' win '` and `'Win'` are the same column. */
    fun normalise(metric: String): String = metric.trim().uppercase()

    /** The subset of [metrics] no extractor implements -- typically a typo. */
    fun unknown(metrics: Collection<String>): List<String> =
        metrics.map(::normalise).distinct().filterNot(extractors::containsKey)

    /** `HEALTH_REMAINING` -> `Health Remaining`, for the leaderboard header. */
    fun label(metric: String): String = normalise(metric)
        .split('_')
        .filter { it.isNotEmpty() }
        .joinToString(" ") { word -> word.lowercase().replaceFirstChar { it.uppercase() } }
}

// The extractors are named functions referenced by `::name` rather than lambdas
// in the map literal: several of them need an early `return`, which a lambda in
// an infix `to` expression cannot express.

private fun appearance(context: MetricContext): Double =
    if (context.role is HeroRole.Played) 1.0 else 0.0

/**
 * A hero its own side banned out. `HeroRole.Banned` stays a bare marker (see
 * the class doc); only these two extractors look past it into the match's
 * `bans` to see which category applied.
 */
private fun selfBan(context: MetricContext): Double {
    if (context.role !is HeroRole.Banned) return 0.0
    val banType = context.match.bans.firstOrNull { it.heroId == context.heroId }?.banType
    return if (banType == BanType.SELF_BAN) 1.0 else 0.0
}

/**
 * A hero the opposing side banned out. `PRE_BAN` -- struck before sides are
 * known -- is priced by neither this nor [selfBan]: nobody chose to deny it
 * to a particular opponent.
 */
private fun opponentBan(context: MetricContext): Double {
    if (context.role !is HeroRole.Banned) return 0.0
    val banType = context.match.bans.firstOrNull { it.heroId == context.heroId }?.banType
    return if (banType == BanType.OPPONENT_BAN) 1.0 else 0.0
}

private fun win(context: MetricContext): Double {
    val participant = context.participant ?: return 0.0
    return if (participant.isWinner) 1.0 else 0.0
}

/**
 * The other side of [win], and exhaustive with it: every game has exactly one
 * winner ([com.umfl.match.MatchRule.NOT_EXACTLY_ONE_WINNER]), so within a game
 * a hero that did not win, lost. There is deliberately no `DRAW` extractor to
 * pair with these -- a drawn game is not a recordable result, so pricing one
 * would be pricing something that cannot happen, and the registry surfaces
 * `DRAW` as an unknown-metric warning instead.
 */
private fun loss(context: MetricContext): Double {
    val participant = context.participant ?: return 0.0
    return if (participant.isWinner) 0.0 else 1.0
}

private fun healthRemaining(context: MetricContext): Double =
    context.participant?.healthRemaining?.toDouble() ?: 0.0

/**
 * This hero's health minus the healthiest opponent's, in a game it won. Unlike
 * [win]/[loss] this is not symmetric across the two sides -- a losing hero
 * scores 0.0 here regardless of how much health it had left, since there is
 * no losing side of this metric to price.
 */
private fun healthDifferential(context: MetricContext): Double {
    val participant = context.participant ?: return 0.0
    if (!participant.isWinner) return 0.0
    val best = context.opponents.maxOfOrNull { it.healthRemaining } ?: return 0.0
    return (participant.healthRemaining - best).toDouble()
}

/**
 * A clean sweep: every opponent finished on zero health. Only awarded to a hero
 * that actually played and that had at least one opponent.
 */
private fun shutout(context: MetricContext): Double {
    context.participant ?: return 0.0
    val opponents = context.opponents
    if (opponents.isEmpty()) return 0.0
    return if (opponents.all { it.healthRemaining == 0 }) 1.0 else 0.0
}
