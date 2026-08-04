package com.umfl.match

enum class MatchRule {
    /** The map is not in this tournament's board pool. */
    MAP_NOT_IN_POOL,

    /** The match does not have exactly the expected number of sides. */
    INVALID_PARTICIPANT_COUNT,

    /** The same hero appears more than once — twice on the table, twice in the bans, or both played and banned. */
    DUPLICATE_HERO,

    /** More than one side is flagged as the winner. */
    MULTIPLE_WINNERS,

    /** A `heroId` referenced by a participant or ban does not exist. */
    UNKNOWN_HERO,
}

data class MatchViolation(val rule: MatchRule, val message: String)

/**
 * [playerLabel] is who piloted the hero, as free text — there is no `player`
 * table to check it against, and nothing scores it. Nothing validates it here
 * for the same reason: any string, including none at all, is a legal answer.
 */
data class MatchParticipantInput(
    val playerLabel: String?,
    val heroId: Long,
    val healthRemaining: Int,
    val isWinner: Boolean,
)

data class MatchBanInput(val heroId: Long)

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
        mapId: Long,
        validMapIds: Set<Long>,
        validHeroIds: Set<Long>,
        participants: List<MatchParticipantInput>,
        bans: List<MatchBanInput>,
        expectedParticipantCount: Int = 2,
    ): List<MatchViolation> = buildList {
        if (mapId !in validMapIds) {
            add(MatchViolation(MatchRule.MAP_NOT_IN_POOL, "Map $mapId is not in this tournament's board pool."))
        }

        val unknownHeroes = (participants.map { it.heroId } + bans.map { it.heroId })
            .filter { it !in validHeroIds }
            .toSortedSet()
        if (unknownHeroes.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.UNKNOWN_HERO,
                    "Hero(es) do not exist: ${unknownHeroes.joinToString()}.",
                )
            )
        }

        if (participants.size != expectedParticipantCount) {
            add(
                MatchViolation(
                    MatchRule.INVALID_PARTICIPANT_COUNT,
                    "Expected $expectedParticipantCount participants but got ${participants.size}.",
                )
            )
        }

        // Across participants *and* bans: a hero cannot be picked twice, banned twice, or
        // banned and then played. The last case is what keeps MatchResult.heroContexts()'
        // "playing wins" tie-break a backstop for bad data rather than a supported input.
        val duplicateHeroes = (participants.map { it.heroId } + bans.map { it.heroId })
            .groupingBy { it }.eachCount().filterValues { it > 1 }.keys
        if (duplicateHeroes.isNotEmpty()) {
            add(
                MatchViolation(
                    MatchRule.DUPLICATE_HERO,
                    "Hero(es) appear more than once: ${duplicateHeroes.sorted().joinToString()}.",
                )
            )
        }

        // Zero winners is the legitimate timed-draw case — only more than one is a violation.
        val winners = participants.count { it.isWinner }
        if (winners > 1) {
            add(
                MatchViolation(
                    MatchRule.MULTIPLE_WINNERS,
                    "Only one side may be recorded as the winner (a timed draw has zero).",
                )
            )
        }
    }
}
