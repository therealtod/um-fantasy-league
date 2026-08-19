package com.umfl.matchimport

/**
 * The scraper sidecar's single-match payload, as produced by
 * `tools/tabletopleague-scraper/scrape-match.mjs`. Field names match that
 * script's JSON exactly — see the "Output shape" section of its README.
 *
 * Only the fields with a home in this domain are modelled. The source carries
 * several more (`seedLabel`, `upset`, `hasAdvantage`, `headToHeadRaw`,
 * `seriesWinner`, `score`, `status`, `title`, `competitionName`) that describe
 * that org's own presentation of the match and have nothing to bind to here;
 * Jackson ignores unknown properties, so they are simply not listed. `roundName`
 * and `seriesFormat` are the exceptions: neither maps to a column, but both are
 * shown to the admin as context while they pick a round number.
 *
 * Everything is nullable because the scraper's extractors return null for
 * anything a selector missed rather than throwing — a partially-parsed match
 * should surface as named unresolved fields, not as a deserialization failure.
 */
data class ScrapedMatch(
    val matchId: String? = null,
    val sourceUrl: String? = null,
    val roundName: String? = null,
    val seriesFormat: String? = null,
    val playedAtRaw: String? = null,
    val sideA: ScrapedSide? = null,
    val sideB: ScrapedSide? = null,
    /** Heroes struck before sides were assigned — `PRE_BAN`, belonging to neither side. */
    val preBans: List<String> = emptyList(),
    val games: List<ScrapedGame> = emptyList(),
)

data class ScrapedSide(
    val playerLabel: String? = null,
    /** Every hero this side drafted, in game order. */
    val picks: List<String> = emptyList(),
    val bans: List<ScrapedBan> = emptyList(),
)

data class ScrapedBan(
    val heroName: String? = null,
    /** Already `PRE_BAN` / `OPPONENT_BAN` / `SELF_BAN` — the scraper emits this repo's vocabulary. */
    val banType: String? = null,
)

data class ScrapedGame(
    val gameIndex: Int? = null,
    val mapName: String? = null,
    val sideA: ScrapedGameSide? = null,
    val sideB: ScrapedGameSide? = null,
)

data class ScrapedGameSide(
    val heroName: String? = null,
    /** Negative for an overkill finish; the loser of a game is always 0 or less. */
    val health: Int? = null,
    val isWinner: Boolean = false,
)
