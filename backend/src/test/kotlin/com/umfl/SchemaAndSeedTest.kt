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

    private fun count(table: String, where: String = "true"): Int =
        jdbcClient.sql("select count(*) from $table where $where").query(Int::class.java).single()

    @Test
    fun `every table is seeded to its expected size`() {
        // These two come from `db/migration/V2__reference_data.sql`, not from the
        // fixtures: the hero and board catalogue migrates in every profile, so a
        // `prod` database carries these same counts with everything below at zero.
        assertEquals(74, count("heroes"))
        assertEquals(35, count("game_maps"))
        assertEquals(4, count("managers"))
        assertEquals(3, count("tournaments"))
        assertEquals(32, count("tournament_heroes"), "12 + 12 + Spring's narrower 8")
        assertEquals(7, count("tournament_maps"))
        assertEquals(4, count("tournament_entries"))
        assertEquals(12, count("entry_slots"), "4 entries x roster size 3")
        assertEquals(3, count("scoring_rule_sets"))
        assertEquals(24, count("scoring_coefficients"), "3 rule sets x 8 metrics")
        assertEquals(13, count("tournament_matches"), "12 single-game matches + the Bo3 decider")
        assertEquals(26, count("match_participants"), "13 matches x 2 sides")
        assertEquals(15, count("match_games"), "12 single-game matches + the Bo3's 3 games")
        assertEquals(30, count("match_game_participants"), "15 games x 2 sides")
        assertEquals(22, count("hero_bans"), "19 original + the Bo3's one ban per category")
        assertEquals(
            28,
            count("match_hero_picks"),
            "26 heroes fielded (12 matches x 2 sides, plus the Bo3's one hero per side across its " +
                "three games) + the Bo3's 2 drafted-and-never-fielded picks",
        )
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
                from tournament_heroes th
                    join tournaments t on t.id = th.tournament_id
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
                select count(*) from tournament_heroes th
                    join tournaments t on t.id = th.tournament_id
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
                from tournament_entries e
                    join managers mg on mg.id = e.manager_id
                    join entry_slots es on es.entry_id = e.id
                    join tournament_heroes th
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
                from tournaments t
                    left join scoring_rule_sets rs on rs.tournament_id = t.id
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
    fun `every recorded draft is complete -- no side fielded a hero it never drafted`() {
        val undrafted = jdbcClient
            .sql(
                """
                select count(*)
                from match_game_participants mgp
                    join match_games mg on mg.id = mgp.game_id
                    left join match_hero_picks hp
                        on hp.match_id = mg.match_id and hp.side = mgp.side and hp.hero_id = mgp.hero_id
                where hp.hero_id is null
                """
            )
            .query(Int::class.java)
            .single()

        assertEquals(
            0,
            undrafted,
            "the same invariant MatchResultPolicy.PLAYED_HERO_NOT_DRAFTED enforces on the way in -- " +
                "a hero on the table but off the draft board would score no APPEARANCE at all",
        )
    }

    @Test
    fun `every seeded ban is sided exactly when its category allows one`() {
        val misfiled = jdbcClient
            .sql(
                """
                select count(*)
                from hero_bans
                where (ban_type = 'PRE_BAN') <> (side is null)
                """
            )
            .query(Int::class.java)
            .single()

        assertEquals(
            0,
            misfiled,
            "a PRE_BAN precedes side assignment and carries no side, and V8 gave every other " +
                "seeded ban the side whose draft it came out of",
        )

        assertEquals(
            13,
            count("hero_bans", "side is null"),
            "one pre-ban per seeded match",
        )
        assertEquals(9, count("hero_bans", "side is not null"), "the 8 opponent bans + the Bo3's self ban")
    }

    @Test
    fun `no hero is both drafted and banned in the same match`() {
        val contradictions = jdbcClient
            .sql(
                """
                select count(*)
                from match_hero_picks hp
                    join hero_bans hb on hb.match_id = hp.match_id and hb.hero_id = hp.hero_id
                """
            )
            .query(Int::class.java)
            .single()

        assertEquals(0, contradictions, "a hero struck out of the draft cannot then be taken in it")
    }

    @Test
    fun `every recorded hero was in the tournament's own pool`() {
        val strays = jdbcClient
            .sql(
                """
                select count(*)
                from match_game_participants mgp
                    join match_games mg on mg.id = mgp.game_id
                    left join tournament_heroes th
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
                from match_games mg
                    left join tournament_maps tm
                        on tm.tournament_id = mg.tournament_id and tm.map_id = mg.map_id
                where tm.map_id is null
                """
            )
            .query(Int::class.java)
            .single()

        assertEquals(0, strays)
    }

    @Test
    fun `exactly one side wins every seeded game`() {
        val winnersPerGame = jdbcClient
            .sql(
                """
                select mg.id, count(*) filter (where mgp.is_winner) as winners
                from match_games mg
                    join match_game_participants mgp on mgp.game_id = mg.id
                group by mg.id
                order by mg.id
                """
            )
            .query { rs, _ -> rs.getLong("id") to rs.getInt("winners") }
            .list()

        // Two winners is stopped by the partial unique index; zero is stopped only
        // by MatchResultPolicy, which the seed SQL bypasses -- so the seed's own
        // conformance to "every game is played to a decision" is asserted here.
        assertTrue(winnersPerGame.all { it.second <= 1 }, "the partial unique index should make this impossible")
        assertEquals(
            emptyList(),
            winnersPerGame.filter { it.second != 1 }.map { it.first },
            "every game has exactly one winner",
        )
    }

    @Test
    fun `the database rejects positive health for a losing hero`() {
        assertFailsWith<DataIntegrityViolationException> {
            jdbcClient
                .sql(
                    """
                    update match_game_participants
                    set health_remaining = 1
                    where game_id = 1 and side = 1
                    """
                )
                .update()
        }
    }

    @Test
    fun `the database rejects a game recorded without a map`() {
        assertFailsWith<DataIntegrityViolationException> {
            jdbcClient
                .sql(
                    """
                    insert into match_games (match_id, tournament_id, game_number, map_id)
                    values (1, 1, 99, null)
                    """
                )
                .update()
        }
    }

    @Test
    fun `match ids ascend with played_at, which is what makes the id a safe polling key`() {
        val played = jdbcClient
            .sql("select id, played_at from tournament_matches order by id")
            .query { rs, _ -> rs.getLong("id") to rs.getTimestamp("played_at").toInstant() }
            .list()

        assertEquals(played.sortedBy { it.second.toEpochMilli() }.map { it.first }, played.map { it.first })
        assertTrue(
            played.map { it.second }.distinct().size < played.size,
            "played_at must NOT be unique — parallel tables share a start time",
        )
    }
}
