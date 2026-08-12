package com.umfl.match

import org.junit.jupiter.api.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class MatchResultPolicyTest {

    private val validMaps = setOf(1L, 2L, 3L)
    private val validHeroes = setOf(10L, 11L, 12L)

    private fun participants(label1: String? = "Someone", label2: String? = "Someone Else") =
        listOf(MatchParticipantInput(playerLabel = label1), MatchParticipantInput(playerLabel = label2))

    private fun gameParticipant(heroId: Long, health: Int = 0, winner: Boolean = false) =
        MatchGameParticipantInput(heroId = heroId, healthRemaining = health, isWinner = winner)

    private fun game(
        number: Int = 1,
        mapId: Long = 1L,
        participants: List<MatchGameParticipantInput>,
    ) = MatchGameInput(gameNumber = number, mapId = mapId, participants = participants)

    private fun oneLegalGame(heroA: Long = 10, heroB: Long = 11) =
        listOf(game(participants = listOf(gameParticipant(heroA, winner = true), gameParticipant(heroB))))

    @Test
    fun `a legal single-game match with a winner has no violations`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = oneLegalGame(),
            bans = emptyList(),
        )

        assertTrue(violations.isEmpty(), "expected no violations but got $violations")
    }

    @Test
    fun `a game with no winner is legal - a timed draw has zero, not more than one`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = listOf(game(participants = listOf(gameParticipant(10), gameParticipant(11)))),
            bans = emptyList(),
        )

        assertTrue(violations.isEmpty(), "expected no violations but got $violations")
    }

    @Test
    fun `a best-of-three series with a hero repeated across games is legal`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = listOf(
                game(1, participants = listOf(gameParticipant(10, winner = true), gameParticipant(11))),
                game(2, participants = listOf(gameParticipant(11, winner = true), gameParticipant(10))),
                game(3, participants = listOf(gameParticipant(10, winner = true), gameParticipant(11))),
            ),
            bans = emptyList(),
        )

        assertTrue(violations.isEmpty(), "expected no violations but got $violations")
    }

    @Test
    fun `a map outside the tournament's pool is rejected, naming the offending game`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = listOf(
                game(1, mapId = 1L, participants = listOf(gameParticipant(10, winner = true), gameParticipant(11))),
                game(2, mapId = 99L, participants = listOf(gameParticipant(11, winner = true), gameParticipant(10))),
            ),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.MAP_NOT_IN_POOL), violations.map { it.rule })
        assertTrue(violations.single().message.contains("[2]"))
    }

    @Test
    fun `too few participants is rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(MatchParticipantInput(playerLabel = "Someone")),
            games = oneLegalGame(),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.INVALID_PARTICIPANT_COUNT), violations.map { it.rule })
    }

    @Test
    fun `too many participants is rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = listOf(
                MatchParticipantInput(playerLabel = "A"),
                MatchParticipantInput(playerLabel = "B"),
                MatchParticipantInput(playerLabel = "C"),
            ),
            games = oneLegalGame(),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.INVALID_PARTICIPANT_COUNT), violations.map { it.rule })
    }

    @Test
    fun `zero games is rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = emptyList(),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.INVALID_GAME_COUNT), violations.map { it.rule })
    }

    @Test
    fun `game numbers with a gap are rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = listOf(
                game(1, participants = listOf(gameParticipant(10, winner = true), gameParticipant(11))),
                game(3, participants = listOf(gameParticipant(11, winner = true), gameParticipant(10))),
            ),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.GAME_NUMBERS_NOT_SEQUENTIAL), violations.map { it.rule })
    }

    @Test
    fun `repeated game numbers are rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = listOf(
                game(1, participants = listOf(gameParticipant(10, winner = true), gameParticipant(11))),
                game(1, participants = listOf(gameParticipant(11, winner = true), gameParticipant(10))),
            ),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.GAME_NUMBERS_NOT_SEQUENTIAL), violations.map { it.rule })
    }

    @Test
    fun `the same hero on both sides within one game is rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = listOf(game(participants = listOf(gameParticipant(10, winner = true), gameParticipant(10)))),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.DUPLICATE_HERO), violations.map { it.rule })
        assertTrue(violations.single().message.contains("[1]"))
    }

    @Test
    fun `a game with the wrong number of sides is rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = listOf(game(participants = listOf(gameParticipant(10, winner = true)))),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.INVALID_GAME_PARTICIPANT_COUNT), violations.map { it.rule })
    }

    @Test
    fun `a hero banned then played in a later game is rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = listOf(
                game(1, participants = listOf(gameParticipant(10, winner = true), gameParticipant(11))),
                game(2, participants = listOf(gameParticipant(12, winner = true), gameParticipant(11))),
            ),
            bans = listOf(MatchBanInput(heroId = 12, banType = BanType.PRE_BAN)),
        )

        assertEquals(listOf(MatchRule.BANNED_HERO_PLAYED), violations.map { it.rule })
        assertTrue(violations.single().message.contains("12"))
    }

    @Test
    fun `the same hero banned twice is rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = oneLegalGame(),
            bans = listOf(
                MatchBanInput(heroId = 12, banType = BanType.PRE_BAN),
                MatchBanInput(heroId = 12, banType = BanType.SELF_BAN),
            ),
        )

        assertEquals(listOf(MatchRule.DUPLICATE_BAN), violations.map { it.rule })
    }

    @Test
    fun `a hero banned in one match and played in another is legal`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = oneLegalGame(),
            bans = listOf(MatchBanInput(heroId = 12, banType = BanType.OPPONENT_BAN)),
        )

        assertTrue(violations.isEmpty(), "expected no violations but got $violations")
    }

    @Test
    fun `two winners in one game is rejected, without tripping a different game's legal single winner`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = listOf(
                game(1, participants = listOf(gameParticipant(10, winner = true), gameParticipant(11, winner = true))),
                game(2, participants = listOf(gameParticipant(11, winner = true), gameParticipant(10))),
            ),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.MULTIPLE_WINNERS), violations.map { it.rule })
        assertTrue(violations.single().message.contains("[1]"))
    }

    @Test
    fun `a nonexistent heroId on a game participant is rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = oneLegalGame(heroA = 10, heroB = 999),
            bans = emptyList(),
        )

        assertEquals(listOf(MatchRule.UNKNOWN_HERO), violations.map { it.rule })
        assertTrue(violations.single().message.contains("999"))
    }

    @Test
    fun `a nonexistent heroId on a ban is rejected`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = oneLegalGame(),
            bans = listOf(MatchBanInput(heroId = 999, banType = BanType.PRE_BAN)),
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
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(label1 = "Nobody On Record", label2 = null),
            games = oneLegalGame(),
            bans = emptyList(),
        )
        assertTrue(violations.isEmpty(), "expected no violations but got $violations")

        val sameLabelTwice = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(label1 = "Tomas Ferreira", label2 = "Tomas Ferreira"),
            games = oneLegalGame(),
            bans = emptyList(),
        )
        assertTrue(sameLabelTwice.isEmpty(), "expected no violations but got $sameLabelTwice")
    }

    @Test
    fun `every broken rule is reported, not just the first`() {
        val violations = MatchResultPolicy.validate(
            validMapIds = validMaps,
            validHeroIds = validHeroes,
            participants = participants(),
            games = listOf(game(mapId = 99L, participants = listOf(gameParticipant(10, winner = true), gameParticipant(10, winner = true)))),
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
