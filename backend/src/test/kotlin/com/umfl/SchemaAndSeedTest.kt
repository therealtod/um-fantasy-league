package com.umfl

import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.TournamentEntryRepository
import com.umfl.tournament.TournamentFormat
import com.umfl.tournament.TournamentRepository
import com.umfl.tournament.TournamentStatus
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.dao.DataIntegrityViolationException
import org.springframework.jdbc.core.simple.JdbcClient
import java.time.LocalDate
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Boots the application against a real PostgreSQL, which runs every Flyway
 * migration. A green run here is the proof that the schema and seed data are
 * valid, not merely well-formed.
 *
 * The counts are exact on purpose: the seed is a fixture that the scoring and
 * standings tests assert against by value, so a row quietly appearing or
 * disappearing has to fail *here* rather than as a mystified arithmetic
 * mismatch three tests later.
 */
class SchemaAndSeedTest @Autowired constructor(
    private val tournamentRepository: TournamentRepository,
    private val entryRepository: TournamentEntryRepository,
    private val jdbcClient: JdbcClient,
) : PostgresIntegrationTest() {

    private fun count(table: String): Int =
        jdbcClient.sql("select count(*) from $table").query(Int::class.java).single()

    @Test
    fun `every table is seeded to its expected size`() {
        assertEquals(74, count("heroes"))
        assertEquals(35, count("game_map"))
        assertEquals(4, count("manager"))
        assertEquals(3, count("tournament"))
        assertEquals(32, count("tournament_hero"), "12 + 12 + Spring's narrower 8")
        assertEquals(7, count("tournament_map"))
        assertEquals(4, count("tournament_entry"))
        assertEquals(12, count("entry_slot"), "4 entries x roster size 3")
        assertEquals(3, count("scoring_rule_set"))
        assertEquals(21, count("scoring_coefficient"), "3 rule sets x 7 metrics")
        assertEquals(13, count("tournament_match"), "12 single-game matches + the Bo3 decider")
        assertEquals(26, count("match_participant"), "13 matches x 2 sides")
        assertEquals(15, count("match_game"), "12 single-game matches + the Bo3's 3 games")
        assertEquals(30, count("match_game_participant"), "15 games x 2 sides")
        assertEquals(22, count("hero_ban"), "19 original + the Bo3's one ban per category")
    }

    @Test
    fun `the three tournaments cover the lifecycle states the lobby renders`() {
        val tournaments = tournamentRepository.findAllByOrderByStartDateAsc()

        assertEquals(
            listOf("Summer of Legends", "Winter of Champions", "Spring of Myths"),
            tournaments.map { it.name },
        )
        assertEquals(
            listOf(TournamentStatus.COMPLETED, TournamentStatus.REGISTRATION_OPEN, TournamentStatus.SCHEDULED),
            tournaments.map { it.status },
        )
        assertEquals(
            listOf(TournamentFormat.BANQUEST, TournamentFormat.ARSENAL, TournamentFormat.BANQUEST),
            tournaments.map { it.format },
        )
        assertTrue(tournaments.all { it.rosterSize == 3 && it.creditGrant == 10_000 })
    }

    @Test
    fun `dates are calendar days and only the finished tournament has an end`() {
        val summer = assertNotNull(tournamentRepository.findByName("Summer of Legends"))
        val winter = assertNotNull(tournamentRepository.findByName("Winter of Champions"))

        assertEquals(LocalDate.parse("2026-06-05"), summer.startDate)
        assertEquals(LocalDate.parse("2026-06-07"), summer.endDate)
        assertEquals(LocalDate.parse("2026-08-14"), winter.startDate)
        assertNull(winter.endDate, "an unfinished tournament has no end date")
    }

    @Test
    fun `hero cost is per tournament, not global`() {
        val costs = jdbcClient
            .sql(
                """
                select t.name as tournament, th.cost
                from tournament_hero th
                    join tournament t on t.id = th.tournament_id
                    join heroes h on h.id = th.hero_id
                where h.name = 'Sun Wukong'
                order by t.name
                """
            )
            .query { rs, _ -> rs.getString("tournament") to rs.getInt("cost") }
            .list()

        assertEquals(
            listOf("Spring of Myths" to 5500, "Summer of Legends" to 5600, "Winter of Champions" to 5300),
            costs,
        )
    }

    @Test
    fun `Spring of Myths carries a narrower pool, so UNKNOWN_HERO is reachable`() {
        val springHeroCount = jdbcClient
            .sql(
                """
                select count(*) from tournament_hero th
                    join tournament t on t.id = th.tournament_id
                where t.name = 'Spring of Myths'
                """
            )
            .query(Int::class.java)
            .single()

        assertEquals(8, springHeroCount)
    }

    @Test
    fun `the finished tournament has four locked rosters inside their grants`() {
        val summer = assertNotNull(tournamentRepository.findByName("Summer of Legends"))
        val entries = entryRepository.findByTournamentId(requireNotNull(summer.id))

        assertEquals(4, entries.size)
        assertTrue(entries.all { it.isLocked })
        assertTrue(entries.all { it.lockedAt != null })
        assertTrue(entries.all { it.slots.size == summer.rosterSize })
        assertTrue(entries.all { it.creditGrant == summer.creditGrant })
    }

    @Test
    fun `every seeded roster is affordable at this tournament's prices`() {
        val spends = jdbcClient
            .sql(
                """
                select mg.handle, sum(th.cost) as spent, e.credit_grant
                from tournament_entry e
                    join manager mg on mg.id = e.manager_id
                    join entry_slot es on es.entry_id = e.id
                    join tournament_hero th
                        on th.tournament_id = e.tournament_id and th.hero_id = es.hero_id
                group by mg.handle, e.credit_grant
                order by mg.handle
                """
            )
            .query { rs, _ -> Triple(rs.getString("handle"), rs.getInt("spent"), rs.getInt("credit_grant")) }
            .list()

        assertEquals(
            listOf(
                Triple("ArthurianLegend", 9_800, 10_000),
                Triple("MythicMind", 9_600, 10_000),
                Triple("NeonStrategist", 9_400, 10_000),
                Triple("SherlockMain", 9_600, 10_000),
            ),
            spends,
        )
    }

    @Test
    fun `the tournament the walkthrough registers for is left empty`() {
        val winter = assertNotNull(tournamentRepository.findByName("Winter of Champions"))

        assertEquals(0, entryRepository.countByTournamentId(requireNotNull(winter.id)))
        assertTrue(winter.acceptsRegistration)
    }

    @Test
    fun `roster slots keep their draft order and never repeat a hero`() {
        val summer = assertNotNull(tournamentRepository.findByName("Summer of Legends"))

        entryRepository.findByTournamentId(requireNotNull(summer.id)).forEach { entry ->
            assertEquals(entry.slots.size, entry.heroIds.distinct().size, "entry ${entry.id}")
        }
    }

    @Test
    fun `exactly one scoring rule set is active per tournament`() {
        val active = jdbcClient
            .sql(
                """
                select t.name, count(rs.id) filter (where rs.is_active) as active_sets
                from tournament t
                    left join scoring_rule_set rs on rs.tournament_id = t.id
                group by t.name
                order by t.name
                """
            )
            .query { rs, _ -> rs.getString("name") to rs.getInt("active_sets") }
            .list()

        assertEquals(
            listOf(
                "Spring of Myths" to 1,
                "Summer of Legends" to 1,
                "Winter of Champions" to 1,
            ),
            active,
        )
    }

    @Test
    fun `every recorded hero was in the tournament's own pool`() {
        val strays = jdbcClient
            .sql(
                """
                select count(*)
                from match_game_participant mgp
                    join match_game mg on mg.id = mgp.game_id
                    left join tournament_hero th
                        on th.tournament_id = mg.tournament_id and th.hero_id = mgp.hero_id
                where th.hero_id is null
                """
            )
            .query(Int::class.java)
            .single()

        assertEquals(
            0,
            strays,
            "a result naming a played hero outside the pool would poison every roster's score -- note " +
                "this checks who *played*, not who was *banned*: match 13's bans deliberately use " +
                "heroes outside Summer of Legends' pool, which this query never looks at",
        )
    }

    @Test
    fun `every recorded board was in the tournament's own map pool`() {
        val strays = jdbcClient
            .sql(
                """
                select count(*)
                from match_game mg
                    left join tournament_map tm
                        on tm.tournament_id = mg.tournament_id and tm.map_id = mg.map_id
                where tm.map_id is null
                """
            )
            .query(Int::class.java)
            .single()

        assertEquals(0, strays)
    }

    @Test
    fun `at most one side wins a game, and one game has no winner at all`() {
        // Grouped by (game id, match id): a game's own bigserial id is what the
        // partial unique index actually constrains, but match_id is what identifies
        // the timed draw below -- match_game.id isn't guaranteed to line up with it.
        val winnersPerGame = jdbcClient
            .sql(
                """
                select mg.id, mg.match_id, count(*) filter (where mgp.is_winner) as winners
                from match_game mg
                    join match_game_participant mgp on mgp.game_id = mg.id
                group by mg.id, mg.match_id
                order by mg.id
                """
            )
            .query { rs, _ -> Triple(rs.getLong("id"), rs.getLong("match_id"), rs.getInt("winners")) }
            .list()

        assertTrue(winnersPerGame.all { it.third <= 1 }, "the partial unique index should make this impossible")
        assertEquals(listOf(11L), winnersPerGame.filter { it.third == 0 }.map { it.second }, "the timed draw")
    }

    @Test
    fun `the database rejects a game recorded without a map`() {
        assertFailsWith<DataIntegrityViolationException> {
            jdbcClient
                .sql(
                    """
                    insert into match_game (match_id, tournament_id, game_number, map_id)
                    values (1, 1, 99, null)
                    """
                )
                .update()
        }
    }

    @Test
    fun `match ids ascend with played_at, which is what makes the id a safe polling key`() {
        val played = jdbcClient
            .sql("select id, played_at from tournament_match order by id")
            .query { rs, _ -> rs.getLong("id") to rs.getTimestamp("played_at").toInstant() }
            .list()

        assertEquals(played.sortedBy { it.second.toEpochMilli() }.map { it.first }, played.map { it.first })
        assertTrue(
            played.map { it.second }.distinct().size < played.size,
            "played_at must NOT be unique — parallel tables share a start time",
        )
    }
}
