package com.umfl.match

enum class MatchRule {
    /** One or more games use a map outside this tournament's board pool. */
    MAP_NOT_IN_POOL,

    /** The series has no games at all — at least one is required. */
    INVALID_GAME_COUNT,

    /** Game numbers aren't exactly 1..N with no gaps or repeats. */
    GAME_NUMBERS_NOT_SEQUENTIAL,

    /** The series does not have exactly the expected number of sides. */
    INVALID_PARTICIPANT_COUNT,

    /** One or more games don't have exactly the expected number of sides. */
    INVALID_GAME_PARTICIPANT_COUNT,

    /** The same hero appears on both sides within one game. */
    DUPLICATE_HERO,

    /** The same hero is banned more than once. */
    DUPLICATE_BAN,

    /** A banned hero was also played, somewhere in the series. */
    BANNED_HERO_PLAYED,

    /**
     * One or more games are not flagged with exactly one winner. Every game is
     * played to a decision — there is no draw in this league, so zero winners
     * is as wrong as two.
     */
    NOT_EXACTLY_ONE_WINNER,

    /** A losing hero finished with positive health. */
    LOSER_HAS_POSITIVE_HEALTH,

    /** A `heroId` referenced by a game participant or a ban does not exist. */
    UNKNOWN_HERO,
}

data class MatchViolation(val rule: MatchRule, val message: String)

/**
 * [playerLabel] is who piloted this side for the whole series, as free text —
 * there is no `player` table to check it against, and nothing scores it.
 * Nothing validates it here for the same reason: any string, including none
 * at all, is a legal answer.
 */
data class MatchParticipantInput(val playerLabel: String?)

data class MatchGameParticipantInput(
    val heroId: Long,
    val healthRemaining: Int,
    val isWinner: Boolean,
)

data class MatchGameInput(
    val gameNumber: Int,
    val mapId: Long,
    val participants: List<MatchGameParticipantInput>,
)

data class MatchBanInput(val heroId: Long, val banType: BanType)

/**
 * Pre-validates a match result before the service attempts to save it, so a
 * bad admin submission comes back as a clear 422 instead of a raw
 * `DataIntegrityViolationException` from a partial unique index or composite
 * foreign key.
 *
 * Deliberately free of Spring and persistence, exactly like
 * [com.umfl.tournament.RosterPolicy] — everything it needs (the pool of legal
 * map ids, and the set of hero ids that actually exist) is resolved by the
 * caller and passed in.
 */
object MatchResultPolicy {

    fun validate(
        validMapIds: Set<Long>,
        validHeroIds: Set<Long>,
        participants: List<MatchParticipantInput>,
        games: List<MatchGameInput>,
        bans: List<MatchBanInput>,
        expectedParticipantCount: Int = 2,
    ): List<MatchViolation> = buildList {
        if (participants.size != expectedParticipantCount) {
            add(
                MatchViolation(
                    MatchRule.INVALID_PARTICIPANT_COUNT,
                    "Expected $expectedParticipantCount participants but got ${participants.size}.",
                )
            )
        }

        if (games.isEmpty()) {
            add(MatchViolation(MatchRule.INVALID_GAME_COUNT, "At least one game is required."))
        } else if (games.map { it.gameNumber }.sorted() != (1..games.size).toList()) {
            add(
                MatchViolation(
                    MatchRule.GAME_NUMBERS_NOT_SEQUENTIAL,
                    "Game numbers must be exactly 1..${games.size} with no gaps or repeats.",
                )
            )
        }

        val badMapGames = games.filter { it.mapId !in validMapIds }.map { it.gameNumber }.sorted()
        if (badMapGames.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.MAP_NOT_IN_POOL,
                    "Game(s) $badMapGames use a map that is not in this tournament's board pool.",
                )
            )
        }

        val badCountGames = games.filter { it.participants.size != expectedParticipantCount }
            .map { it.gameNumber }.sorted()
        if (badCountGames.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.INVALID_GAME_PARTICIPANT_COUNT,
                    "Game(s) $badCountGames don't have exactly $expectedParticipantCount sides.",
                )
            )
        }

        val sameHeroGames = games
            .filter { game -> game.participants.map { it.heroId }.distinct().size != game.participants.size }
            .map { it.gameNumber }.sorted()
        if (sameHeroGames.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.DUPLICATE_HERO,
                    "Game(s) $sameHeroGames have the same hero on both sides.",
                )
            )
        }

        val duplicateBans = bans.map { it.heroId }.groupingBy { it }.eachCount().filterValues { it > 1 }.keys
        if (duplicateBans.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.DUPLICATE_BAN,
                    "Hero(es) banned more than once: ${duplicateBans.sorted().joinToString()}.",
                )
            )
        }

        val playedHeroIds = games.flatMapTo(mutableSetOf()) { game -> game.participants.map { it.heroId } }
        val bannedButPlayed = bans.map { it.heroId }.filter { it in playedHeroIds }.toSortedSet()
        if (bannedButPlayed.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.BANNED_HERO_PLAYED,
                    "Hero(es) both banned and played somewhere in this series: ${bannedButPlayed.joinToString()}.",
                )
            )
        }

        val undecidedGames = games.filter { game -> game.participants.count { it.isWinner } != 1 }
            .map { it.gameNumber }.sorted()
        if (undecidedGames.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.NOT_EXACTLY_ONE_WINNER,
                    "Game(s) $undecidedGames do not have exactly one winner. Every game is " +
                        "played to a decision, so a game with no winner is as invalid as one with two.",
                )
            )
        }

        val survivingLoserGames = games
            .filter { game -> game.participants.any { !it.isWinner && it.healthRemaining > 0 } }
            .map { it.gameNumber }.sorted()
        if (survivingLoserGames.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.LOSER_HAS_POSITIVE_HEALTH,
                    "Losing hero(es) in game(s) $survivingLoserGames must have 0 or less health.",
                )
            )
        }

        val allHeroIds = games.flatMap { game -> game.participants.map { it.heroId } } + bans.map { it.heroId }
        val unknownHeroes = allHeroIds.filter { it !in validHeroIds }.toSortedSet()
        if (unknownHeroes.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.UNKNOWN_HERO,
                    "Hero(es) do not exist: ${unknownHeroes.joinToString()}.",
                )
            )
        }
    }
}
