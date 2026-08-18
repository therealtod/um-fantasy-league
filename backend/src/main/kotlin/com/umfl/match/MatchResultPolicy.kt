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

    /** The same hero is drafted more than once by one side. */
    DUPLICATE_PICK,

    /** A banned hero was also played, somewhere in the series. */
    BANNED_HERO_PLAYED,

    /** A banned hero was also drafted — it was struck before either side could take it. */
    BANNED_HERO_DRAFTED,

    /**
     * A hero played a game for a side that never drafted it. A recorded draft
     * is the complete list of what a side brought, so every hero it fielded
     * has to be on it.
     */
    PLAYED_HERO_NOT_DRAFTED,

    /**
     * One or more games are not flagged with exactly one winner. Every game is
     * played to a decision — there is no draw in this league, so zero winners
     * is as wrong as two.
     */
    NOT_EXACTLY_ONE_WINNER,

    /** A losing hero finished with positive health. */
    LOSER_HAS_POSITIVE_HEALTH,

    /** A `heroId` referenced by a game participant, a pick or a ban does not exist. */
    UNKNOWN_HERO,
}

data class MatchViolation(val rule: MatchRule, val message: String)

/**
 * [playerLabel] is who piloted this side for the whole series, as free text —
 * there is no `player` table to check it against, and nothing scores it.
 * Nothing validates it here for the same reason: any string, including none
 * at all, is a legal answer.
 *
 * [draftedHeroIds] is this side's half of the match's draft — every hero it
 * took, whether or not it fielded one. The side is the position of this input
 * in the participants list, exactly as it already is for `player_label`, so an
 * out-of-range side is unrepresentable and there is no rule policing one.
 */
data class MatchParticipantInput(
    val playerLabel: String?,
    val draftedHeroIds: List<Long> = emptyList(),
)

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

        val duplicatePicks = participants
            .flatMap { participant ->
                participant.draftedHeroIds.groupingBy { it }.eachCount().filterValues { it > 1 }.keys
            }
            .toSortedSet()
        if (duplicatePicks.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.DUPLICATE_PICK,
                    "Hero(es) drafted more than once by one side: ${duplicatePicks.joinToString()}.",
                )
            )
        }

        val draftedHeroIds = participants.flatMapTo(mutableSetOf()) { it.draftedHeroIds }
        val bannedButDrafted = bans.map { it.heroId }.filter { it in draftedHeroIds }.toSortedSet()
        if (bannedButDrafted.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.BANNED_HERO_DRAFTED,
                    "Hero(es) both banned and drafted in this series: ${bannedButDrafted.joinToString()}.",
                )
            )
        }

        // `side` is the list position on both sides of this check -- of the
        // participants list for the draft, and of a game's participants list
        // for who fielded the hero. A game with the wrong number of sides is
        // INVALID_GAME_PARTICIPANT_COUNT's problem, so an unmatched index here
        // is simply skipped rather than reported twice.
        val undraftedPlays = games
            .flatMap { game ->
                game.participants.withIndex().mapNotNull { (side, played) ->
                    val draft = participants.getOrNull(side)?.draftedHeroIds ?: return@mapNotNull null
                    if (played.heroId in draft) null else game.gameNumber to played.heroId
                }
            }
            .toSortedSet(compareBy({ it.first }, { it.second }))
        if (undraftedPlays.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.PLAYED_HERO_NOT_DRAFTED,
                    "Hero(es) played by a side that did not draft them: " +
                        undraftedPlays.joinToString { (gameNumber, heroId) -> "$heroId in game $gameNumber" } +
                        ". A recorded draft lists every hero a side brought, fielded or not.",
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

        val allHeroIds = games.flatMap { game -> game.participants.map { it.heroId } } +
            participants.flatMap { it.draftedHeroIds } +
            bans.map { it.heroId }
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
