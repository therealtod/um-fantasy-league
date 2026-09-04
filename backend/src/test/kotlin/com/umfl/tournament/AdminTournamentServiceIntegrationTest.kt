package com.umfl.tournament

import com.umfl.common.ConflictException
import com.umfl.manager.ManagerRepository
import com.umfl.support.PostgresIntegrationTest
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.jdbc.core.simple.JdbcClient
import java.time.LocalDate
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class AdminTournamentServiceIntegrationTest @Autowired constructor(
    private val adminTournamentService: AdminTournamentService,
    private val tournamentService: TournamentService,
    private val tournamentRepository: TournamentRepository,
    private val entryRepository: TournamentEntryRepository,
    private val managerRepository: ManagerRepository,
    private val jdbcClient: JdbcClient,
) : PostgresIntegrationTest() {

    private fun heroId(name: String): Long =
        jdbcClient.sql("select id from heroes where name = :name").param("name", name).query(Long::class.java).single()

    @Test
    fun `creates a new tournament`() {
        val created = adminTournamentService.create(
            name = "Autumn Invitational",
            format = TournamentFormat.ARSENAL,
            status = TournamentStatus.SCHEDULED,
            startDate = LocalDate.parse("2026-10-01"),
            endDate = null,
            capacity = 32,
            rosterSize = 3,
            creditGrant = 10_000,
        )

        assertNotNull(created.id)
        assertNotNull(tournamentRepository.findByName("Autumn Invitational"))
    }

    @Test
    fun `creating a tournament with a name already in use is rejected`() {
        assertFailsWith<ConflictException> {
            adminTournamentService.create(
                name = "Winter of Champions",
                format = TournamentFormat.ARSENAL,
                status = TournamentStatus.SCHEDULED,
                startDate = LocalDate.parse("2026-10-01"),
                endDate = null,
                capacity = 32,
                rosterSize = 3,
                creditGrant = 10_000,
            )
        }
    }

    @Test
    fun `updates an existing tournament, including moving it to the next lifecycle status`() {
        val spring = requireNotNull(tournamentRepository.findByName("Spring of Myths"))

        val updated = adminTournamentService.update(
            tournamentId = requireNotNull(spring.id),
            name = spring.name,
            format = spring.format,
            status = TournamentStatus.REGISTRATION_OPEN,
            startDate = spring.startDate,
            endDate = spring.endDate,
            capacity = 40,
            rosterSize = spring.rosterSize,
            creditGrant = spring.creditGrant,
        )

        assertEquals(TournamentStatus.REGISTRATION_OPEN, updated.status)
        assertEquals(40, updated.capacity)
        assertEquals(TournamentStatus.REGISTRATION_OPEN, requireNotNull(tournamentRepository.findByName("Spring of Myths")).status)
    }

    @Test
    fun `renaming a tournament to a name already in use is rejected`() {
        val spring = requireNotNull(tournamentRepository.findByName("Spring of Myths"))

        assertFailsWith<ConflictException> {
            adminTournamentService.update(
                tournamentId = requireNotNull(spring.id),
                name = "Winter of Champions",
                format = spring.format,
                status = spring.status,
                startDate = spring.startDate,
                endDate = spring.endDate,
                capacity = spring.capacity,
                rosterSize = spring.rosterSize,
                creditGrant = spring.creditGrant,
            )
        }
    }

    @Test
    fun `deleting a tournament removes it and allows creating a new one with the same name`() {
        val spring = requireNotNull(tournamentRepository.findByName("Spring of Myths"))
        val springId = requireNotNull(spring.id)

        // Delete the tournament
        adminTournamentService.delete(springId)

        // Verify it's gone
        assertNull(tournamentRepository.findByName("Spring of Myths"))
        assertNull(tournamentRepository.findById(springId).orElse(null))

        // Should be able to create a new tournament with the same name
        val newTournament = adminTournamentService.create(
            name = "Spring of Myths",
            format = TournamentFormat.BANQUEST,
            status = TournamentStatus.SCHEDULED,
            startDate = LocalDate.parse("2026-09-18"),
            endDate = null,
            capacity = 32,
            rosterSize = 3,
            creditGrant = 10_000,
        )

        assertNotNull(newTournament.id)
        assertNotNull(tournamentRepository.findByName("Spring of Myths"))
        assertNotEquals(springId, newTournament.id, "Should be a new tournament with a new ID")
    }

    @Test
    fun `deleting a tournament with related data cascades properly`() {
        val summer = requireNotNull(tournamentRepository.findByName("Summer of Legends"))
        val summerId = requireNotNull(summer.id)

        // Verify there's related data before deletion
        val entryCount = jdbcClient.sql("select count(*) from tournament_entries where tournament_id = :id")
            .param("id", summerId).query(Int::class.java).single()
        val matchCount = jdbcClient.sql("select count(*) from tournament_matches where tournament_id = :id")
            .param("id", summerId).query(Int::class.java).single()

        assertTrue(entryCount > 0, "Summer of Legends should have entries")
        assertTrue(matchCount > 0, "Summer of Legends should have matches")

        // Delete the tournament
        adminTournamentService.delete(summerId)

        // Verify tournament is gone
        assertNull(tournamentRepository.findByName("Summer of Legends"))

        // Verify related data is gone (cascade delete)
        val entriesAfter = jdbcClient.sql("select count(*) from tournament_entries where tournament_id = :id")
            .param("id", summerId).query(Int::class.java).single()
        val matchesAfter = jdbcClient.sql("select count(*) from tournament_matches where tournament_id = :id")
            .param("id", summerId).query(Int::class.java).single()
        val heroPoolAfter = jdbcClient.sql("select count(*) from tournament_heroes where tournament_id = :id")
            .param("id", summerId).query(Int::class.java).single()
        val mapPoolAfter = jdbcClient.sql("select count(*) from tournament_maps where tournament_id = :id")
            .param("id", summerId).query(Int::class.java).single()
        val ruleSetsAfter = jdbcClient.sql("select count(*) from scoring_rule_sets where tournament_id = :id")
            .param("id", summerId).query(Int::class.java).single()

        assertEquals(0, entriesAfter, "Tournament entries should be cascade deleted")
        assertEquals(0, matchesAfter, "Matches should be cascade deleted")
        assertEquals(0, heroPoolAfter, "Hero pool should be cascade deleted")
        assertEquals(0, mapPoolAfter, "Map pool should be cascade deleted")
        assertEquals(0, ruleSetsAfter, "Scoring rule sets should be cascade deleted")
    }

    @Test
    fun `moving a tournament to LIVE purges every entry that never locked a roster`() {
        val spring = requireNotNull(tournamentRepository.findByName("Spring of Myths"))
        val springId = requireNotNull(spring.id)

        adminTournamentService.update(
            tournamentId = springId,
            name = spring.name,
            format = spring.format,
            status = TournamentStatus.REGISTRATION_OPEN,
            startDate = spring.startDate,
            endDate = spring.endDate,
            capacity = spring.capacity,
            rosterSize = spring.rosterSize,
            creditGrant = spring.creditGrant,
        )

        val lockedManager = requireNotNull(managerRepository.findByHandle("NeonStrategist"))
        val neverLockedManager = requireNotNull(managerRepository.findByHandle("SherlockMain"))

        tournamentService.register(springId, lockedManager)
        tournamentService.setSlots(
            springId,
            lockedManager,
            listOf(heroId("Sherlock Holmes"), heroId("Beowulf"), heroId("Sinbad")),
        )
        tournamentService.lockRoster(springId, lockedManager)

        tournamentService.register(springId, neverLockedManager)
        tournamentService.setSlots(springId, neverLockedManager, listOf(heroId("Alice")))
        // Deliberately never locked.

        adminTournamentService.update(
            tournamentId = springId,
            name = spring.name,
            format = spring.format,
            status = TournamentStatus.LIVE,
            startDate = spring.startDate,
            endDate = spring.endDate,
            capacity = spring.capacity,
            rosterSize = spring.rosterSize,
            creditGrant = spring.creditGrant,
        )

        val remaining = entryRepository.findByTournamentId(springId)
        assertEquals(1, remaining.size, "the never-locked entry was purged when the tournament went live")
        assertEquals(lockedManager.id, remaining.single().managerId)
        assertTrue(remaining.single().isLocked)
    }

    @Test
    fun `re-entering LIVE purges an entry registered while registration was briefly reopened`() {
        val spring = requireNotNull(tournamentRepository.findByName("Spring of Myths"))
        val springId = requireNotNull(spring.id)

        fun moveTo(status: TournamentStatus) = adminTournamentService.update(
            tournamentId = springId,
            name = spring.name,
            format = spring.format,
            status = status,
            startDate = spring.startDate,
            endDate = spring.endDate,
            capacity = spring.capacity,
            rosterSize = spring.rosterSize,
            creditGrant = spring.creditGrant,
        )

        moveTo(TournamentStatus.REGISTRATION_OPEN)
        moveTo(TournamentStatus.LIVE)

        // Registration is reopened after the tournament already went live once.
        moveTo(TournamentStatus.REGISTRATION_OPEN)
        val lateRegistrant = requireNotNull(managerRepository.findByHandle("MythicMind"))
        tournamentService.register(springId, lateRegistrant)
        // Never locked before the tournament goes live again.

        moveTo(TournamentStatus.LIVE)

        assertTrue(
            entryRepository.findByTournamentId(springId).isEmpty(),
            "the late, never-locked registration is purged the second time the tournament goes live too",
        )
    }
}
