package com.umfl.scoring

import com.umfl.match.BanResult
import com.umfl.match.BanType
import com.umfl.match.DraftedHeroResult
import com.umfl.match.GameParticipantResult
import com.umfl.match.GameResult
import com.umfl.match.MatchParticipantResult
import com.umfl.match.MatchResult
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import java.time.Instant
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * A per-metric truth table over hand-built matches.
 *
 * The fixtures mirror the shapes the seed data actually contains — a decided
 * match, the round-2 shutout, the round-3 game won on health with both heroes
 * alive, and (in [Games]) the round-3 best-of-three — so a metric that
 * disagrees with reality fails here rather than three layers up in the
 * leaderboard.
 */
class MatchMetricsTest {

    private val played = Instant.parse("2026-06-06T11:00:00Z")

    private fun gameParticipant(
        side: Int,
        heroId: Long,
        heroName: String,
        health: Int,
        isWinner: Boolean = false,
    ) = GameParticipantResult(
        side = side,
        heroId = heroId,
        heroName = heroName,
        healthRemaining = health,
        isWinner = isWinner,
    )

    /**
     * A single-game match: one `GameResult` wrapping [gameParticipants].
     *
     * Every side drafts the hero it fielded, since a recorded draft is complete
     * (`MatchRule.PLAYED_HERO_NOT_DRAFTED`). [unplayedPicks] adds heroes side 0
     * drafted and never played — the case APPEARANCE exists to catch.
     */
    private fun match(
        gameParticipants: List<GameParticipantResult>,
        bans: List<BanResult> = emptyList(),
        unplayedPicks: List<DraftedHeroResult> = emptyList(),
    ) = MatchResult(
        matchId = 6,
        tournamentId = 1,
        round = 2,
        playedAt = played,
        externalLink = "https://example.com/match/metrics",
        participants = gameParticipants.map { participant ->
            MatchParticipantResult(
                side = participant.side,
                playerLabel = "Player ${participant.side}",
                draftedHeroes = listOf(DraftedHeroResult(participant.heroId, participant.heroName)) +
                    if (participant.side == 0) unplayedPicks else emptyList(),
            )
        },
        games = listOf(
            GameResult(gameId = 1, gameNumber = 1, mapId = 3, mapName = "Raptor Paddock", participants = gameParticipants),
        ),
        bans = bans,
    )

    /** Match 6 of the seed: Bigfoot 11 beats Beowulf 0. */
    private val shutoutMatch = match(
        listOf(
            gameParticipant(side = 0, heroId = 7, heroName = "Bigfoot", health = 11, isWinner = true),
            gameParticipant(side = 1, heroId = 11, heroName = "Beowulf", health = 0),
        ),
        bans = listOf(BanResult(heroId = 8, heroName = "Sun Wukong", banType = BanType.PRE_BAN)),
    )

    /** Match 11 of the seed: Sherlock Holmes wins 7 to Dracula's 0. */
    private val decisiveMatch = match(
        listOf(
            gameParticipant(side = 0, heroId = 5, heroName = "Sherlock Holmes", health = 7, isWinner = true),
            gameParticipant(side = 1, heroId = 6, heroName = "Dracula", health = 0),
        ),
    )

    /** A fixture with one of each ban category, to tell SELF_BAN/OPPONENT_BAN apart. */
    private val banVarietyMatch = match(
        listOf(
            gameParticipant(side = 0, heroId = 1, heroName = "Alice", health = 5, isWinner = true),
            gameParticipant(side = 1, heroId = 2, heroName = "Medusa", health = 0),
        ),
        bans = listOf(
            BanResult(heroId = 20, heroName = "Invisible Man", banType = BanType.SELF_BAN),
            BanResult(heroId = 21, heroName = "Robin Hood", banType = BanType.OPPONENT_BAN),
        ),
    )

    /**
     * The hero's played-or-banned context — the per-game one for a hero that
     * played a single-game match, the ban marker for one that did not. Its
     * `Drafted` context is deliberately excluded: a drafted hero that played
     * has both, and every per-game metric is measured on the [HeroRole.Played]
     * one.
     */
    private fun contextFor(match: MatchResult, heroId: Long): MetricContext =
        assertNotNull(
            match.heroContexts().singleOrNull { it.heroId == heroId && it.role !is HeroRole.Drafted },
            "no unique play-or-ban context for hero $heroId",
        )

    /** The hero's once-per-series `Drafted` context — what APPEARANCE prices. */
    private fun draftContextFor(match: MatchResult, heroId: Long): MetricContext =
        assertNotNull(
            match.heroContexts().singleOrNull { it.heroId == heroId && it.role is HeroRole.Drafted },
            "no draft context for hero $heroId",
        )

    private fun measure(metric: String, context: MetricContext): Double {
        val extractor = assertNotNull(MatchMetrics[metric], "no extractor for $metric")
        return extractor(context)
    }

    @Nested
    inner class Registry {

        @Test
        fun `every advertised metric resolves to an extractor`() {
            assertEquals(
                setOf(
                    "APPEARANCE", "SELF_BAN", "OPPONENT_BAN", "WIN", "LOSS",
                    "HEALTH_REMAINING", "HEALTH_DIFFERENTIAL", "SHUTOUT",
                ),
                MatchMetrics.known,
            )
        }

        @Test
        fun `lookup is case- and whitespace-insensitive`() {
            assertNotNull(MatchMetrics[" win "])
            assertNotNull(MatchMetrics["Health_Remaining"])
        }

        @Test
        fun `unknown catches a typo and leaves the real keys alone`() {
            assertEquals(
                listOf("HEALTH_REMAINNG", "CROWD_FAVOURITE"),
                MatchMetrics.unknown(listOf("WIN", "HEALTH_REMAINNG", "SELF_BAN", "CROWD_FAVOURITE")),
            )
        }

        @Test
        fun `DRAW is not a metric - a game with no winner is not a recordable result`() {
            // Pricing DRAW would be pricing something MatchResultPolicy rejects, so
            // it is warned about like any other metric this build cannot measure.
            assertNull(MatchMetrics["DRAW"])
            assertEquals(listOf("DRAW"), MatchMetrics.unknown(listOf("WIN", "DRAW", "LOSS")))
        }

        @Test
        fun `an unimplemented metric resolves to nothing rather than throwing`() {
            assertNull(MatchMetrics["CROWD_FAVOURITE"])
        }

        @Test
        fun `labels are title-cased for the leaderboard header`() {
            assertEquals("Health Remaining", MatchMetrics.label("HEALTH_REMAINING"))
            assertEquals("Win", MatchMetrics.label("win"))
            assertEquals("Health Differential", MatchMetrics.label("HEALTH_DIFFERENTIAL"))
        }
    }

    @Nested
    inner class Outcomes {

        @Test
        fun `the winner takes WIN and not LOSS`() {
            val bigfoot = contextFor(shutoutMatch, heroId = 7)

            assertEquals(1.0, measure("WIN", bigfoot))
            assertEquals(0.0, measure("LOSS", bigfoot))
        }

        @Test
        fun `the defeated hero takes LOSS only`() {
            val beowulf = contextFor(shutoutMatch, heroId = 11)

            assertEquals(0.0, measure("WIN", beowulf))
            assertEquals(1.0, measure("LOSS", beowulf))
        }

        @Test
        fun `WIN and LOSS are exhaustive within a decided game`() {
            val sherlock = contextFor(decisiveMatch, heroId = 5)
            val dracula = contextFor(decisiveMatch, heroId = 6)

            assertEquals(1.0, measure("WIN", sherlock))
            assertEquals(0.0, measure("LOSS", sherlock))
            assertEquals(0.0, measure("WIN", dracula))
            assertEquals(1.0, measure("LOSS", dracula))
        }
    }

    @Nested
    inner class Health {

        @Test
        fun `HEALTH_REMAINING is the hero's own end-of-game health`() {
            assertEquals(11.0, measure("HEALTH_REMAINING", contextFor(shutoutMatch, 7)))
            assertEquals(0.0, measure("HEALTH_REMAINING", contextFor(shutoutMatch, 11)))
            assertEquals(0.0, measure("HEALTH_REMAINING", contextFor(decisiveMatch, 6)))
        }

        @Test
        fun `HEALTH_DIFFERENTIAL is won-gated -- only the winner scores it`() {
            val sherlock = measure("HEALTH_DIFFERENTIAL", contextFor(decisiveMatch, 5))
            val dracula = measure("HEALTH_DIFFERENTIAL", contextFor(decisiveMatch, 6))

            assertEquals(7.0, sherlock)
            assertEquals(0.0, dracula, "the losing side has no differential to price")
        }

        @Test
        fun `HEALTH_DIFFERENTIAL measures the winner against the healthiest opponent`() {
            val threeWay = match(
                listOf(
                    gameParticipant(side = 0, heroId = 1, heroName = "Alice", health = 6, isWinner = true),
                    gameParticipant(side = 1, heroId = 2, heroName = "Medusa", health = 4),
                    gameParticipant(side = 2, heroId = 3, heroName = "Sinbad", health = 0),
                ),
            )

            assertEquals(2.0, measure("HEALTH_DIFFERENTIAL", contextFor(threeWay, 1)))
            assertEquals(0.0, measure("HEALTH_DIFFERENTIAL", contextFor(threeWay, 2)))
            assertEquals(0.0, measure("HEALTH_DIFFERENTIAL", contextFor(threeWay, 3)))
        }
    }

    @Nested
    inner class Shutout {

        @Test
        fun `a shutout needs every opponent on zero`() {
            assertEquals(1.0, measure("SHUTOUT", contextFor(shutoutMatch, 7)))
        }

        @Test
        fun `the shut-out hero does not itself earn a shutout`() {
            assertEquals(0.0, measure("SHUTOUT", contextFor(shutoutMatch, 11)))
        }

        @Test
        fun `a win with the opponent still alive is no shutout`() {
            val narrow = match(
                listOf(
                    gameParticipant(side = 0, heroId = 4, heroName = "Medusa", health = 8, isWinner = true),
                    gameParticipant(side = 1, heroId = 8, heroName = "Sun Wukong", health = 2),
                ),
            )

            assertEquals(0.0, measure("SHUTOUT", contextFor(narrow, 4)))
        }

        @Test
        fun `one surviving opponent out of two denies the shutout`() {
            val threeWay = match(
                listOf(
                    gameParticipant(side = 0, heroId = 1, heroName = "Alice", health = 6, isWinner = true),
                    gameParticipant(side = 1, heroId = 2, heroName = "Medusa", health = 1),
                    gameParticipant(side = 2, heroId = 3, heroName = "Sinbad", health = 0),
                ),
            )

            assertEquals(0.0, measure("SHUTOUT", contextFor(threeWay, 1)))
        }
    }

    @Nested
    inner class Bans {

        @Test
        fun `a pre-banned hero earns neither SELF_BAN nor OPPONENT_BAN`() {
            val sunWukong = contextFor(shutoutMatch, heroId = 8)

            assertEquals(0.0, measure("SELF_BAN", sunWukong))
            assertEquals(0.0, measure("OPPONENT_BAN", sunWukong))
            assertEquals(0.0, measure("APPEARANCE", sunWukong))
            assertEquals(0.0, measure("WIN", sunWukong))
            assertEquals(0.0, measure("LOSS", sunWukong))
            assertEquals(0.0, measure("HEALTH_REMAINING", sunWukong))
            assertEquals(0.0, measure("HEALTH_DIFFERENTIAL", sunWukong))
            assertEquals(0.0, measure("SHUTOUT", sunWukong))
        }

        @Test
        fun `a self-banned hero earns SELF_BAN and nothing else`() {
            val invisibleMan = contextFor(banVarietyMatch, heroId = 20)

            assertEquals(1.0, measure("SELF_BAN", invisibleMan))
            assertEquals(0.0, measure("OPPONENT_BAN", invisibleMan))
            assertEquals(0.0, measure("APPEARANCE", invisibleMan))
        }

        @Test
        fun `an opponent-banned hero earns OPPONENT_BAN and nothing else`() {
            val robinHood = contextFor(banVarietyMatch, heroId = 21)

            assertEquals(1.0, measure("OPPONENT_BAN", robinHood))
            assertEquals(0.0, measure("SELF_BAN", robinHood))
            assertEquals(0.0, measure("APPEARANCE", robinHood))
        }

        @Test
        fun `a hero that played earns APPEARANCE off its draft and never SELF_BAN or OPPONENT_BAN`() {
            val bigfootDraft = draftContextFor(shutoutMatch, heroId = 7)
            val bigfootPlayed = contextFor(shutoutMatch, heroId = 7)

            assertEquals(1.0, measure("APPEARANCE", bigfootDraft))
            assertEquals(0.0, measure("APPEARANCE", bigfootPlayed), "appearance is a draft fact, not a game one")
            assertEquals(0.0, measure("SELF_BAN", bigfootDraft))
            assertEquals(0.0, measure("OPPONENT_BAN", bigfootDraft))
        }

        @Test
        fun `heroContexts covers every hero a single-game match touched`() {
            val contexts = shutoutMatch.heroContexts()

            // Both heroes played and were drafted; Sun Wukong was banned out.
            assertEquals(listOf(7L, 11L, 7L, 11L, 8L), contexts.map { it.heroId })
            assertEquals(3, contexts.distinctBy { it.heroId }.size)
            assertTrue(contexts.single { it.heroId == 8L }.role is HeroRole.Banned)
        }

        @Test
        fun `a banned hero's role is a bare marker -- its category lives on the ban, not the role`() {
            val role = contextFor(shutoutMatch, heroId = 8).role

            assertEquals(HeroRole.Banned, role)
            assertEquals(BanType.PRE_BAN, shutoutMatch.bans.single { it.heroId == 8L }.banType)
        }
    }

    @Nested
    inner class Draft {

        /** Match 6 again, but side 0 also drafted a hero it never fielded. */
        private val benchedMatch = match(
            listOf(
                gameParticipant(side = 0, heroId = 7, heroName = "Bigfoot", health = 11, isWinner = true),
                gameParticipant(side = 1, heroId = 11, heroName = "Beowulf", health = 0),
            ),
            bans = listOf(BanResult(heroId = 8, heroName = "Sun Wukong", banType = BanType.PRE_BAN)),
            unplayedPicks = listOf(DraftedHeroResult(heroId = 30, heroName = "Tomoe Gozen")),
        )

        @Test
        fun `a hero drafted and never fielded still earns APPEARANCE`() {
            val tomoe = draftContextFor(benchedMatch, heroId = 30)

            assertEquals(1.0, measure("APPEARANCE", tomoe))
        }

        @Test
        fun `a hero drafted and never fielded earns nothing a game would have given it`() {
            val tomoe = draftContextFor(benchedMatch, heroId = 30)

            assertEquals(0.0, measure("WIN", tomoe))
            assertEquals(0.0, measure("LOSS", tomoe))
            assertEquals(0.0, measure("HEALTH_REMAINING", tomoe))
            assertEquals(0.0, measure("HEALTH_DIFFERENTIAL", tomoe))
            assertEquals(0.0, measure("SHUTOUT", tomoe))
            assertEquals(0.0, measure("SELF_BAN", tomoe))
            assertEquals(0.0, measure("OPPONENT_BAN", tomoe))
        }

        @Test
        fun `an unfielded pick yields one context, and it is not a game`() {
            val contexts = benchedMatch.heroContexts().filter { it.heroId == 30L }

            assertEquals(1, contexts.size)
            assertEquals(HeroRole.Drafted, contexts.single().role)
            assertNull(contexts.single().participant, "a hero that never played has no participant row")
        }

        @Test
        fun `a banned hero is never also drafted, so it earns no APPEARANCE`() {
            val sunWukong = contextFor(benchedMatch, heroId = 8)

            assertEquals(0.0, measure("APPEARANCE", sunWukong))
            assertTrue(benchedMatch.heroContexts().none { it.heroId == 8L && it.role is HeroRole.Drafted })
        }
    }

    @Nested
    inner class Games {

        /**
         * The seed's Bo3 (match 13) with only its first two games played: Medusa
         * takes game 1, Achilles takes game 2 back. The point of the fixture is
         * that the same two heroes appear twice with opposite outcomes, so a
         * metric folded per *series* instead of per *game* cannot pass.
         */
        private val bestOfTwoSoFar = MatchResult(
            matchId = 13,
            tournamentId = 1,
            round = 3,
            playedAt = played,
            externalLink = "https://challonge.com/example-bo3-decider",
            participants = listOf(
                MatchParticipantResult(0, "Rina Okafor", listOf(DraftedHeroResult(4, "Medusa"))),
                MatchParticipantResult(1, "Dmitri Kovac", listOf(DraftedHeroResult(20, "Achilles"))),
            ),
            games = listOf(
                GameResult(
                    gameId = 101,
                    gameNumber = 1,
                    mapId = 2,
                    mapName = "Sherwood Forest",
                    participants = listOf(
                        gameParticipant(side = 0, heroId = 4, heroName = "Medusa", health = 6, isWinner = true),
                        gameParticipant(side = 1, heroId = 20, heroName = "Achilles", health = 0),
                    ),
                ),
                GameResult(
                    gameId = 102,
                    gameNumber = 2,
                    mapId = 3,
                    mapName = "Raptor Paddock",
                    participants = listOf(
                        gameParticipant(side = 0, heroId = 4, heroName = "Medusa", health = 0),
                        gameParticipant(side = 1, heroId = 20, heroName = "Achilles", health = 5, isWinner = true),
                    ),
                ),
            ),
            bans = emptyList(),
        )

        private fun playedContexts(match: MatchResult, heroId: Long) =
            match.heroContexts().filter { it.heroId == heroId && it.role is HeroRole.Played }

        @Test
        fun `a hero played in two games of a series yields two Played contexts, each scoring its own game`() {
            val medusaContexts = playedContexts(bestOfTwoSoFar, heroId = 4)

            assertEquals(2, medusaContexts.size)
            val (game1, game2) = medusaContexts
            assertEquals(6.0, measure("HEALTH_REMAINING", game1))
            assertEquals(0.0, measure("HEALTH_REMAINING", game2))
        }

        @Test
        fun `WIN and LOSS are scoped per game, not per series`() {
            val (medusaGame1, medusaGame2) = playedContexts(bestOfTwoSoFar, heroId = 4)
            val (achillesGame1, achillesGame2) = playedContexts(bestOfTwoSoFar, heroId = 20)

            // Game 1: Medusa took it, Achilles lost it.
            assertEquals(1.0, measure("WIN", medusaGame1))
            assertEquals(0.0, measure("LOSS", medusaGame1))
            assertEquals(0.0, measure("WIN", achillesGame1))
            assertEquals(1.0, measure("LOSS", achillesGame1))

            // Game 2: the other way round, in the same series. Each hero ends the
            // series holding one WIN and one LOSS rather than a single verdict.
            assertEquals(0.0, measure("WIN", medusaGame2))
            assertEquals(1.0, measure("LOSS", medusaGame2))
            assertEquals(1.0, measure("WIN", achillesGame2))
            assertEquals(0.0, measure("LOSS", achillesGame2))
        }

        @Test
        fun `a banned hero still yields exactly one Banned context regardless of how many games the series has`() {
            val threeGameSeries = bestOfTwoSoFar.copy(
                games = bestOfTwoSoFar.games + GameResult(
                    gameId = 103,
                    gameNumber = 3,
                    mapId = 1,
                    mapName = "Baskerville Manor",
                    participants = listOf(
                        gameParticipant(side = 0, heroId = 4, heroName = "Medusa", health = 3, isWinner = true),
                        gameParticipant(side = 1, heroId = 20, heroName = "Achilles", health = 0),
                    ),
                ),
                bans = listOf(BanResult(heroId = 6, heroName = "Bruce Lee", banType = BanType.PRE_BAN)),
            )

            val banContexts = threeGameSeries.heroContexts().filter { it.heroId == 6L }
            assertEquals(1, banContexts.size, "a ban is struck once, before any game -- it must not multiply by game count")
            assertTrue(banContexts.single().role is HeroRole.Banned)
        }

        @Test
        fun `a hero that played every game of a Bo3 is still drafted once, so APPEARANCE is not multiplied`() {
            val threeGameSeries = bestOfTwoSoFar.copy(
                games = bestOfTwoSoFar.games + GameResult(
                    gameId = 103,
                    gameNumber = 3,
                    mapId = 1,
                    mapName = "Baskerville Manor",
                    participants = listOf(
                        gameParticipant(side = 0, heroId = 4, heroName = "Medusa", health = 3, isWinner = true),
                        gameParticipant(side = 1, heroId = 20, heroName = "Achilles", health = 0),
                    ),
                ),
            )

            assertEquals(3, playedContexts(threeGameSeries, heroId = 4).size)
            val draftContexts = threeGameSeries.heroContexts().filter { it.heroId == 4L && it.role is HeroRole.Drafted }
            assertEquals(1, draftContexts.size, "a draft happens once for the series, like a ban")
            assertEquals(1.0, measure("APPEARANCE", draftContexts.single()))
        }
    }
}
