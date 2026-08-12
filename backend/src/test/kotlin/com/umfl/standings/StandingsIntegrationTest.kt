package com.umfl.standings

import com.umfl.manager.Manager
import com.umfl.manager.ManagerRepository
import com.umfl.manager.RankDivision
import com.umfl.manager.RankTier
import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.EntryStatus
import com.umfl.tournament.TournamentEntry
import com.umfl.tournament.TournamentEntryRepository
import com.umfl.tournament.TournamentRepository
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.jdbc.core.simple.JdbcClient
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * The standings fold, against the recorded results in the seed.
 *
 * The seed is static, so every assertion here is exact rather than "something is
 * nonzero". The arithmetic is reproduced in the comments for the row that is
 * hand-verified end to end; the rest follow from the same weights.
 *
 * Weights (`Season 2026 Standard`): WIN 10, HEALTH_REMAINING 0.75,
 * HEALTH_DIFFERENTIAL 0.5, SHUTOUT 3, BAN 2, APPEARANCE 1 — plus the
 * deliberately unimplemented CROWD_FAVOURITE 5.
 */
class StandingsIntegrationTest @Autowired constructor(
    private val standingsService: StandingsService,
    private val standingsQuery: StandingsQuery,
    private val tournamentRepository: TournamentRepository,
    private val entryRepository: TournamentEntryRepository,
    private val managerRepository: ManagerRepository,
    private val jdbcClient: JdbcClient,
) : PostgresIntegrationTest() {

    private fun summerId(): Long =
        requireNotNull(assertNotNull(tournamentRepository.findByName("Summer of Legends")).id)

    private fun winterId(): Long =
        requireNotNull(assertNotNull(tournamentRepository.findByName("Winter of Champions")).id)

    private fun board() = standingsService.board(summerId())

    private fun row(handle: String) = assertNotNull(board().rows.singleOrNull { it.handle == handle })

    @Test
    fun `the board carries the rule set, the round and its own columns`() {
        val board = board()

        assertEquals(summerId(), board.tournamentId)
        assertEquals("Season 2026 Standard", board.ruleSetName)
        assertEquals(3, board.currentRound)
        assertEquals(
            listOf("WIN", "HEALTH_REMAINING", "HEALTH_DIFFERENTIAL", "SHUTOUT", "BAN", "APPEARANCE"),
            board.metrics.map { it.metric },
            "column order is sort_order, which only the database knows",
        )
        assertEquals(
            listOf("Win", "Health Remaining", "Health Differential", "Shutout", "Ban", "Appearance"),
            board.metrics.map { it.label },
        )
        assertEquals(listOf(10.0, 0.75, 0.5, 3.0, 2.0, 1.0), board.metrics.map { it.coefficient })
    }

    @Test
    fun `the seeded CROWD_FAVOURITE metric is weighted but scores nothing anywhere`() {
        val configured = jdbcClient
            .sql(
                """
                select count(*) from scoring_coefficient c
                    join scoring_rule_set rs on rs.id = c.rule_set_id
                where rs.tournament_id = :t and c.metric = 'CROWD_FAVOURITE'
                """
            )
            .param("t", summerId())
            .query(Int::class.java)
            .single()
        assertEquals(1, configured, "precondition: it really is configured, at a coefficient of 5")

        val board = board()

        assertFalse(board.metrics.any { it.metric == "CROWD_FAVOURITE" }, "it must not become a column")
        board.rows.forEach { row ->
            assertFalse("CROWD_FAVOURITE" in row.breakdown, "${row.handle} scored an unimplemented metric")
        }
    }

    @Test
    fun `the leaderboard is exact, ordered and complete`() {
        val board = board()

        assertEquals(
            listOf(
                "ArthurianLegend" to 84.00,
                "NeonStrategist" to 75.75,
                "MythicMind" to 63.25,
                "SherlockMain" to 51.50,
            ),
            board.rows.map { it.handle to it.totalPoints },
        )
        assertEquals(listOf(1, 2, 3, 4), board.rows.map { it.rank })
    }

    /**
     * The row worked out by hand, metric by metric.
     *
     * King Arthur — m3 loss (HR 4x0.75=3.00, HD (4-5)x0.5=-0.50, APP 1.00);
     *   m8 win (WIN 10, HR 10x0.75=7.50, HD (10-3)x0.5=3.50, APP 1.00);
     *   m12 banned (BAN 2.00).
     * Yennenga — m3 win (WIN 10, HR 3.75, HD 0.50, APP 1.00);
     *   m11 banned (BAN 2.00); m12 win (WIN 10, HR 6.75, HD 3.00, APP 1.00).
     * Beowulf — m3 banned (BAN 2.00); m6 shut out (HR 0, HD (0-11)x0.5=-5.50,
     *   APP 1.00); m8 banned (BAN 2.00); m10 win (WIN 10, HR 6.00, HD 2.00,
     *   APP 1.00).
     *
     * WIN 40.00 + HEALTH_REMAINING 27.00 + HEALTH_DIFFERENTIAL 3.00
     *   + SHUTOUT 0.00 + BAN 8.00 + APPEARANCE 6.00 = 84.00
     */
    @Test
    fun `the winning row adds up metric by metric`() {
        val arthurian = row("ArthurianLegend")

        assertEquals(1, arthurian.rank)
        assertEquals(listOf("King Arthur", "Yennenga", "Beowulf"), arthurian.roster)
        assertEquals(9_800, arthurian.spent)
        assertEquals(10_000, arthurian.creditGrant)
        assertEquals(
            mapOf(
                "WIN" to 40.0,
                "HEALTH_REMAINING" to 27.0,
                "HEALTH_DIFFERENTIAL" to 3.0,
                "SHUTOUT" to 0.0,
                "BAN" to 8.0,
                "APPEARANCE" to 6.0,
            ),
            arthurian.breakdown,
        )
        assertEquals(84.0, arthurian.totalPoints)
    }

    /** The only roster holding the shutout hero, so the only one with SHUTOUT points. */
    @Test
    fun `the shutout shows up on exactly the rosters that drafted Bigfoot`() {
        val board = board()

        assertEquals(
            mapOf(
                "ArthurianLegend" to 0.0,
                "NeonStrategist" to 3.0,
                "MythicMind" to 3.0,
                "SherlockMain" to 0.0,
            ),
            board.rows.associate { it.handle to (it.breakdown["SHUTOUT"] ?: 0.0) },
            "Bigfoot's single shutout is counted once into each of the two rosters holding it",
        )
    }

    @Test
    fun `a total is exactly the sum of its own breakdown`() {
        board().rows.forEach { row ->
            assertEquals(
                row.totalPoints,
                com.umfl.scoring.ScoringEngine.round2(row.breakdown.values.sum()),
                "${row.handle}: displayed total must equal the sum of the displayed parts",
            )
        }
    }

    @Test
    fun `round points are the latest round's swing, not the whole total`() {
        val board = board()

        assertEquals(
            listOf(
                "ArthurianLegend" to 43.75,
                "NeonStrategist" to 8.00,
                "MythicMind" to 6.25,
                "SherlockMain" to 30.00,
            ),
            board.rows.map { it.handle to it.roundPoints },
        )
        assertTrue(
            board.rows.all { it.roundPoints <= it.totalPoints },
            "round 3 is a subset of rounds 1-3",
        )
    }

    @Test
    fun `an entry with no picks still appears, and ties share a rank`() {
        val summer = summerId()
        listOf("EmptyDrafterA", "EmptyDrafterB").forEach { handle ->
            val manager = managerRepository.save(
                Manager(
                    handle = handle,
                    displayName = handle,
                    rankTier = RankTier.BRONZE,
                    rankDivision = RankDivision.III,
                )
            )
            entryRepository.save(
                TournamentEntry(
                    tournamentId = summer,
                    managerId = requireNotNull(manager.id),
                    status = EntryStatus.DRAFT,
                    creditGrant = 10_000,
                )
            )
        }

        val board = standingsService.board(summer)

        assertEquals(6, board.rows.size, "a left join keeps the pickless entries on the board")
        val empties = board.rows.filter { it.handle.startsWith("EmptyDrafter") }
        assertEquals(2, empties.size)
        assertTrue(empties.all { it.roster.isEmpty() && it.spent == 0 && it.totalPoints == 0.0 })
        // Standard competition ranking: 1, 2, 3, 4, 5, 5 — not 1..6.
        assertEquals(listOf(1, 2, 3, 4, 5, 5), board.rows.map { it.rank })
    }

    @Test
    fun `a tournament with entries but no matches scores everyone zero`() {
        val winter = winterId()
        val manager = assertNotNull(managerRepository.findByHandle("NeonStrategist"))
        entryRepository.save(
            TournamentEntry(
                tournamentId = winter,
                managerId = requireNotNull(manager.id),
                status = EntryStatus.DRAFT,
                creditGrant = 10_000,
            )
        )

        val board = standingsService.board(winter)

        assertEquals(0, board.currentRound)
        assertEquals(1, board.rows.size)
        assertEquals(0.0, board.rows.single().totalPoints)
        assertEquals(6, board.metrics.size, "the columns exist even before a single match is played")
    }

    @Test
    fun `a tournament with no active rule set still returns a usable board`() {
        val summer = summerId()
        jdbcClient.sql("delete from scoring_rule_set where tournament_id = :t").param("t", summer).update()

        val board = standingsService.board(summer)

        assertEquals("", board.ruleSetName)
        assertEquals(3, board.currentRound, "the rounds were still played")
        assertTrue(board.metrics.isEmpty())
        assertEquals(4, board.rows.size)
        assertTrue(board.rows.all { it.totalPoints == 0.0 && it.breakdown.isEmpty() })
    }

    @Test
    fun `the roster projection carries slot order and live prices`() {
        val rosters = standingsQuery.rosters(summerId())

        val neon = assertNotNull(rosters.singleOrNull { it.handle == "NeonStrategist" })
        assertEquals(listOf(0, 1, 2), neon.heroes.map { it.slotIndex })
        assertEquals(listOf("Alice", "Robin Hood", "Bigfoot"), neon.heroes.map { it.name })
        assertEquals(listOf(4_100, 3_200, 2_100), neon.heroes.map { it.cost })
        assertEquals(9_400, neon.spent)
    }

    @Test
    fun `the ticker returns every match newest first`() {
        val ticker = standingsService.ticker(summerId(), sinceMatchId = 0, limit = 25)

        assertEquals(13, ticker.size)
        assertEquals(
            listOf(13L, 12L, 11L, 10L, 9L, 8L, 7L, 6L, 5L, 4L, 3L, 2L, 1L),
            ticker.map { it.matchId },
            "ordered by played_at desc, then id desc for the parallel tables",
        )
        assertEquals(listOf(3, 3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1), ticker.map { it.round })
    }

    @Test
    fun `polling on the match id returns only what is new`() {
        val fresh = standingsService.ticker(summerId(), sinceMatchId = 10, limit = 25)

        assertEquals(listOf(13L, 12L, 11L), fresh.map { it.matchId })
        assertTrue(fresh.all { it.matchId > 10 })
    }

    @Test
    fun `a ticker entry names both sides, winner first, with their net points`() {
        val shutout = assertNotNull(standingsService.ticker(summerId()).singleOrNull { it.matchId == 6L })

        assertEquals(1, shutout.games.size, "a single-game match has exactly one game entry")
        val game = shutout.games.single()
        assertEquals("Raptor Paddock", game.mapName)
        assertEquals(listOf("Bigfoot", "Beowulf"), game.sides.map { it.heroName })
        assertEquals(listOf("Aurelie Blanc", "Miles Ashworth"), game.sides.map { it.playerLabel })
        assertEquals(listOf(true, false), game.sides.map { it.isWinner })
        assertEquals(listOf(11, 0), game.sides.map { it.healthRemaining })
        // 10 + 8.25 + 5.50 + 3 + 1 = 27.75 against 0 + (-5.50) + 1 = -4.50.
        assertEquals(listOf(27.75, -4.5), game.sides.map { it.points })
        assertEquals(listOf("Sun Wukong"), shutout.bannedHeroNames)
    }

    @Test
    fun `a timed draw has no winner and still scores both sides`() {
        val draw = assertNotNull(standingsService.ticker(summerId()).singleOrNull { it.matchId == 11L })

        val game = draw.games.single()
        assertTrue(game.sides.none { it.isWinner })
        assertEquals(setOf("Sherlock Holmes", "Dracula"), game.sides.map { it.heroName }.toSet())
        assertEquals(
            mapOf("Sherlock Holmes" to 7.25, "Dracula" to 3.75),
            game.sides.associate { it.heroName to it.points },
        )
        assertEquals(listOf("Medusa", "Yennenga"), draw.bannedHeroNames)
    }

    /**
     * Match 13's Bo3 (Medusa vs. Achilles) is the seed's proof that a
     * multi-game series records and scores correctly end to end -- but
     * neither hero sits on any manager's roster, so every point it generates
     * is folded into `appearancesByHero` and never picked up by `board()`.
     * The totals here must be identical to the single-hand-verified-row
     * fixture in `the winning row adds up metric by metric`.
     */
    @Test
    fun `match 13, the Bo3 decider, is recorded and scored but changes no roster's total`() {
        val decider = assertNotNull(standingsService.ticker(summerId()).singleOrNull { it.matchId == 13L })

        assertEquals(3, decider.games.size, "the Bo3 recorded all three games")
        assertEquals("https://challonge.com/example-bo3-decider", decider.externalLink)
        assertEquals(listOf(1, 2, 3), decider.games.map { it.gameNumber })
        assertTrue(decider.games.all { game -> game.sides.map { it.heroName }.toSet() == setOf("Medusa", "Achilles") })

        val board = board()
        assertTrue(board.rows.all { "Medusa" !in it.roster && "Achilles" !in it.roster })
        assertEquals(
            listOf(
                "ArthurianLegend" to 84.00,
                "NeonStrategist" to 75.75,
                "MythicMind" to 63.25,
                "SherlockMain" to 51.50,
            ),
            board.rows.map { it.handle to it.totalPoints },
        )
    }
}
