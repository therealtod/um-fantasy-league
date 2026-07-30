package com.umfl.match

import org.junit.jupiter.api.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class MatchResultPolicyTest {

    private val validMaps = setOf(1L, 2L, 3L)
    private val validHeroes = setOf(10L, 11L, 12L)

    private fun participant(
        heroId: Long,
        health: Int = 5,
        winner: Boolean = false,
        label: String? = "Someone",
    ) = MatchParticipantInput(
        playerLabel = label,
        heroId = heroId,
        healthRemaining = health,
        isWinner = winner,
    )

    @Test
    fun `a legal two-sided match with a winner has no violations`() {
        val violations = MatchResultPolicy.validate(
            mapId = 1L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(participant(10, winner = true), participant(11)),
            bans = emptyList(),
        )

        assertTrue(violations.isEmpty(), "expected no violations but got $violations")
    }

    @Test
    fun `a timed draw with no winner is legal`() {
        val violations = MatchResultPolicy.validate(
            mapId = 1L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(participant(10), participant(11)),
            bans = emptyList(),
        )

        assertTrue(violations.isEmpty(), "expected no violations but got $violations")
    }

    @Test
    fun `a map outside the tournament's pool is rejected`() {
        val violations = MatchResultPolicy.validate(
            mapId = 99L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(participant(10, winner = true), participant(11)),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.MAP_NOT_IN_POOL), violations.map { it.rule })
    }

    @Test
    fun `too few participants is rejected`() {
        val violations = MatchResultPolicy.validate(
            mapId = 1L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(participant(10, winner = true)),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.INVALID_PARTICIPANT_COUNT), violations.map { it.rule })
    }

    @Test
    fun `too many participants is rejected`() {
        val violations = MatchResultPolicy.validate(
            mapId = 1L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(participant(10, winner = true), participant(11), participant(12)),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.INVALID_PARTICIPANT_COUNT), violations.map { it.rule })
    }

    @Test
    fun `the same hero twice is rejected`() {
        val violations = MatchResultPolicy.validate(
            mapId = 1L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(participant(10, winner = true), participant(10)),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.DUPLICATE_HERO), violations.map { it.rule })
    }

    @Test
    fun `two winners is rejected`() {
        val violations = MatchResultPolicy.validate(
            mapId = 1L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(participant(10, winner = true), participant(11, winner = true)),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.MULTIPLE_WINNERS), violations.map { it.rule })
    }

    @Test
    fun `a nonexistent heroId is rejected`() {
        val violations = MatchResultPolicy.validate(
            mapId = 1L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(participant(10, winner = true), participant(999)),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.UNKNOWN_HERO), violations.map { it.rule })
        assertTrue(violations.single().message.contains("999"))
    }

    @Test
    fun `a nonexistent heroId on a ban is rejected`() {
        val violations = MatchResultPolicy.validate(
            mapId = 1L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(participant(10, winner = true), participant(11)),
            bans = listOf(MatchBanInput(heroId = 999)),
        )

        assertEquals(listOf(MatchRule.UNKNOWN_HERO), violations.map { it.rule })
        assertTrue(violations.single().message.contains("999"))
    }

    /**
     * The player label is free text with no table behind it, so there is nothing
     * to check it against — any string, a duplicate, or none at all is legal.
     * This is the guard against someone quietly reintroducing a `player` entity.
     */
    @Test
    fun `player labels are never validated - arbitrary, duplicate, blank and absent all pass`() {
        val violations = MatchResultPolicy.validate(
            mapId = 1L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(
                participant(10, winner = true, label = "Nobody On Record"),
                participant(11, label = null),
            ),
            bans = emptyList(),
        )
        assertTrue(violations.isEmpty(), "expected no violations but got $violations")

        val sameLabelTwice = MatchResultPolicy.validate(
            mapId = 1L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(
                participant(10, winner = true, label = "Tomas Ferreira"),
                participant(11, label = "Tomas Ferreira"),
            ),
            bans = emptyList(),
        )
        assertTrue(sameLabelTwice.isEmpty(), "expected no violations but got $sameLabelTwice")
    }

    @Test
    fun `every broken rule is reported, not just the first`() {
        val violations = MatchResultPolicy.validate(
            mapId = 99L,
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(
                participant(10, winner = true),
                participant(10, winner = true),
            ),
            bans = emptyList(),
        )

        assertEquals(
            setOf(
                MatchRule.MAP_NOT_IN_POOL,
                MatchRule.DUPLICATE_HERO,
                MatchRule.MULTIPLE_WINNERS,
            ),
            violations.map { it.rule }.toSet(),
        )
    }
}
