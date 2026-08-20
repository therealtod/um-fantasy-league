package com.umfl

import com.umfl.hero.HeroPoolAdminRepository
import com.umfl.manager.ManagerRepository
import com.umfl.map.MapPoolAdminRepository
import com.umfl.match.AdminMatchService
import com.umfl.match.MatchGameInput
import com.umfl.match.MatchGameParticipantInput
import com.umfl.match.MatchParticipantInput
import com.umfl.scoring.AdminScoringService
import com.umfl.scoring.ScoringCoefficientInput
import com.umfl.standings.StandingsService
import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.AdminTournamentService
import com.umfl.tournament.EntryStatus
import com.umfl.tournament.TournamentEntryRepository
import com.umfl.tournament.TournamentFormat
import com.umfl.tournament.TournamentService
import com.umfl.tournament.TournamentStatus
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.jdbc.core.simple.JdbcClient
import java.math.BigDecimal
import java.time.Instant
import java.time.LocalDate
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * The whole real-world journey in one test: an admin stands up a brand new
 * tournament (pool, board, scoring), four managers register and draft inside
 * budget, the admin locks in live results round by round, and the tournament
 * closes with an unambiguous winner on the standings board.
 *
 * Every step goes through the same service the corresponding controller
 * calls — this is deliberately not a walkthrough of any single feature
 * package, which is why it lives at the top level next to
 * [SchemaAndSeedTest].
 *
 * The four rosters are a clean partition of the seed's twelve heroes (three
 * apiece, no overlaps) so every match's result can only ever credit one
 * manager's total — the arithmetic below is exact, not "something increased".
 */
class TournamentLifecycleIntegrationTest @Autowired constructor(
    private val adminTournamentService: AdminTournamentService,
    private val heroPoolAdminRepository: HeroPoolAdminRepository,
    private val mapPoolAdminRepository: MapPoolAdminRepository,
    private val adminScoringService: AdminScoringService,
    private val tournamentService: TournamentService,
    private val adminMatchService: AdminMatchService,
    private val standingsService: StandingsService,
    private val entryRepository: TournamentEntryRepository,
    private val managerRepository: ManagerRepository,
    private val jdbcClient: JdbcClient,
) : PostgresIntegrationTest() {

    private fun manager(handle: String) = assertNotNull(managerRepository.findByHandle(handle))

    private fun id(table: String, name: String): Long =
        jdbcClient.sql("select id from $table where name = :name").param("name", name).query(Long::class.java).single()

    private fun heroId(name: String) = id("heroes", name)
    private fun mapId(name: String) = id("game_map", name)

    @Test
    fun `a tournament runs from creation to a decided winner`() {
        // --- An admin stands up the tournament, closed to registration at first. ---
        val tournament = adminTournamentService.create(
            name = "Autumn Championship",
            format = TournamentFormat.ARSENAL,
            status = TournamentStatus.SCHEDULED,
            startDate = LocalDate.parse("2026-10-01"),
            endDate = null,
            capacity = 8,
            rosterSize = 3,
            creditGrant = 10_000,
        )
        val tournamentId = requireNotNull(tournament.id)

        // --- Pool, board and scoring are configured before anyone can draft. ---
        val allHeroes = listOf(
            "Alice", "King Arthur", "Robin Hood", "Medusa", "Sherlock Holmes", "Dracula",
            "Bigfoot", "Sun Wukong", "Achilles", "Yennenga", "Beowulf", "Sinbad",
        )
        allHeroes.forEach { heroPoolAdminRepository.upsertCost(tournamentId, heroId(it), 1_000) }
        listOf("Baskerville Manor", "Sherwood Forest", "Raptor Paddock")
            .forEach { mapPoolAdminRepository.addToPool(tournamentId, mapId(it)) }

        val ruleSet = adminScoringService.create(
            tournamentId = tournamentId,
            name = "Autumn Standard",
            coefficients = listOf(
                ScoringCoefficientInput("WIN", BigDecimal("10"), sortOrder = 0),
                ScoringCoefficientInput("HEALTH_REMAINING", BigDecimal("1"), sortOrder = 1),
                ScoringCoefficientInput("APPEARANCE", BigDecimal("1"), sortOrder = 2),
            ),
            activate = true,
        )
        assertTrue(ruleSet.ruleSet.isActive)

        // --- Registration opens. ---
        adminTournamentService.update(
            tournamentId = tournamentId,
            name = tournament.name,
            format = tournament.format,
            status = TournamentStatus.REGISTRATION_OPEN,
            startDate = tournament.startDate,
            endDate = tournament.endDate,
            capacity = tournament.capacity,
            rosterSize = tournament.rosterSize,
            creditGrant = tournament.creditGrant,
        )

        // --- Four managers register and draft a clean, disjoint partition of the pool. ---
        val rosters = mapOf(
            "SherlockMain" to listOf("Alice", "King Arthur", "Robin Hood"),
            "MythicMind" to listOf("Medusa", "Sherlock Holmes", "Dracula"),
            "NeonStrategist" to listOf("Bigfoot", "Sun Wukong", "Achilles"),
            "ArthurianLegend" to listOf("Yennenga", "Beowulf", "Sinbad"),
        )
        rosters.forEach { (handle, heroNames) ->
            tournamentService.register(tournamentId, manager(handle))
            val snapshot = tournamentService.setSlots(tournamentId, manager(handle), heroNames.map(::heroId))
            assertEquals(3_000, snapshot.budget.spent, "$handle: three heroes at 1,000 apiece")
            val locked = tournamentService.lockRoster(tournamentId, manager(handle))
            assertEquals(EntryStatus.LOCKED, locked.entry.status)
        }

        // --- Matches are live; rosters are frozen. ---
        adminTournamentService.update(
            tournamentId = tournamentId,
            name = tournament.name,
            format = tournament.format,
            status = TournamentStatus.LIVE,
            startDate = tournament.startDate,
            endDate = tournament.endDate,
            capacity = tournament.capacity,
            rosterSize = tournament.rosterSize,
            creditGrant = tournament.creditGrant,
        )

        fun record(round: Int, map: String, winner: Pair<String, Pair<String, Int>>, loser: Pair<String, Pair<String, Int>>) {
            val (winnerPlayer, winnerPick) = winner
            val (winnerHero, winnerHealth) = winnerPick
            val (loserPlayer, loserPick) = loser
            val (loserHero, loserHealth) = loserPick
            adminMatchService.record(
                tournamentId = tournamentId,
                round = round,
                playedAt = Instant.now(),
                externalLink = "https://example.com/match/round-$round-$winnerPlayer",
                participants = listOf(
                    MatchParticipantInput(winnerPlayer, listOf(heroId(winnerHero))),
                    MatchParticipantInput(loserPlayer, listOf(heroId(loserHero))),
                ),
                games = listOf(
                    MatchGameInput(
                        gameNumber = 1,
                        mapId = mapId(map),
                        participants = listOf(
                            MatchGameParticipantInput(heroId(winnerHero), winnerHealth, true),
                            MatchGameParticipantInput(heroId(loserHero), loserHealth, false),
                        ),
                    ),
                ),
                bans = emptyList(),
            )
        }

        // Round 1.
        record(1, "Baskerville Manor", "Tomas Ferreira" to ("Alice" to 8), "Rina Okafor" to ("Medusa" to 0))
        record(1, "Sherwood Forest", "Hana Sato" to ("Yennenga" to 5), "Dmitri Kovac" to ("Bigfoot" to 0))
        // Round 2.
        record(2, "Raptor Paddock", "Aurelie Blanc" to ("King Arthur" to 10), "Miles Ashworth" to ("Sun Wukong" to 0))
        record(2, "Baskerville Manor", "Jonas Lindqvist" to ("Beowulf" to 6), "Priya Raghunathan" to ("Sherlock Holmes" to 0))

        // --- The tournament ends. ---
        adminTournamentService.update(
            tournamentId = tournamentId,
            name = tournament.name,
            format = tournament.format,
            status = TournamentStatus.COMPLETED,
            startDate = tournament.startDate,
            endDate = LocalDate.parse("2026-10-03"),
            capacity = tournament.capacity,
            rosterSize = tournament.rosterSize,
            creditGrant = tournament.creditGrant,
        )

        // --- The board resolves a single, unambiguous winner. ---
        val board = standingsService.board(tournamentId)

        assertEquals(4, board.rows.size)
        assertEquals(2, board.currentRound)
        assertEquals(
            listOf(
                "SherlockMain" to 40.0,
                "ArthurianLegend" to 33.0,
                "MythicMind" to 2.0,
                "NeonStrategist" to 2.0,
            ),
            board.rows.map { it.handle to it.totalPoints },
            """
            SherlockMain -- Alice (win 10 + health 8 + appearance 1 = 19) + King Arthur (win 10 + health 10 + appearance 1 = 21) = 40
            ArthurianLegend -- Yennenga (10+5+1=16) + Beowulf (10+6+1=17) = 33
            MythicMind -- Medusa (0+0+1=1) + Sherlock Holmes (0+0+1=1) = 2
            NeonStrategist -- Bigfoot (0+0+1=1) + Sun Wukong (0+0+1=1) = 2
            """.trimIndent(),
        )
        assertEquals(listOf(1, 2, 3, 3), board.rows.map { it.rank })

        val winner = board.rows.single { it.rank == 1 }
        assertEquals("SherlockMain", winner.handle)
        assertEquals(listOf("Alice", "King Arthur", "Robin Hood"), winner.roster)

        assertEquals(
            TournamentStatus.COMPLETED,
            requireNotNull(jdbcClient.sql("select status from tournament where id = :id").param("id", tournamentId)
                .query(String::class.java).single()).let(TournamentStatus::valueOf),
        )
        assertTrue(entryRepository.findByTournamentId(tournamentId).all { it.isLocked }, "results only ever landed on locked rosters")
    }
}
