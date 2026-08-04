package com.umfl.map

import com.umfl.common.ConflictException
import com.umfl.common.NotFoundException
import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.TournamentRepository
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class AdminMapServiceIntegrationTest @Autowired constructor(
    private val adminMapService: AdminMapService,
    private val gameMapRepository: GameMapRepository,
    private val mapPoolAdminRepository: MapPoolAdminRepository,
    private val tournamentRepository: TournamentRepository,
) : PostgresIntegrationTest() {

    private fun winterOfChampionsId() = requireNotNull(tournamentRepository.findByName("Winter of Champions")?.id)
    private fun summerOfLegendsId() = requireNotNull(tournamentRepository.findByName("Summer of Legends")?.id)

    @Test
    fun `creates a new map`() {
        val created = adminMapService.create("Sunken Ruins")

        assertNotNull(created.id)
        assertNotNull(gameMapRepository.findByName("Sunken Ruins"))
    }

    @Test
    fun `creating a map with a name already in use is rejected`() {
        assertFailsWith<ConflictException> { adminMapService.create("Baskerville Manor") }
    }

    @Test
    fun `renames an existing map`() {
        val created = adminMapService.create("Placeholder Arena")

        val updated = adminMapService.update(requireNotNull(created.id), "Renamed Arena")

        assertEquals("Renamed Arena", updated.name)
    }

    @Test
    fun `adds a map to a tournament's board pool`() {
        val tournamentId = winterOfChampionsId()
        val raptorPaddock = requireNotNull(gameMapRepository.findByName("Raptor Paddock")?.id)
        assertTrue(raptorPaddock !in mapPoolAdminRepository.poolMapIds(tournamentId), "precondition: not yet in Winter's pool")

        adminMapService.addToPool(tournamentId, raptorPaddock)

        assertTrue(raptorPaddock in mapPoolAdminRepository.poolMapIds(tournamentId))
    }

    @Test
    fun `adding the same map to a pool twice is idempotent`() {
        val tournamentId = winterOfChampionsId()
        val raptorPaddock = requireNotNull(gameMapRepository.findByName("Raptor Paddock")?.id)

        adminMapService.addToPool(tournamentId, raptorPaddock)
        adminMapService.addToPool(tournamentId, raptorPaddock)

        assertEquals(1, mapPoolAdminRepository.poolMapIds(tournamentId).count { it == raptorPaddock })
    }

    @Test
    fun `poolMaps reflects a newly added map by name`() {
        val tournamentId = winterOfChampionsId()
        val raptorPaddock = requireNotNull(gameMapRepository.findByName("Raptor Paddock")?.id)
        assertTrue(mapPoolAdminRepository.poolMaps(tournamentId).none { it.name == "Raptor Paddock" })

        adminMapService.addToPool(tournamentId, raptorPaddock)

        assertTrue(mapPoolAdminRepository.poolMaps(tournamentId).any { it.name == "Raptor Paddock" })
    }

    @Test
    fun `removes a map from a tournament's pool`() {
        val tournamentId = winterOfChampionsId()
        val raptorPaddock = requireNotNull(gameMapRepository.findByName("Raptor Paddock")?.id)
        adminMapService.addToPool(tournamentId, raptorPaddock)

        adminMapService.removeFromPool(tournamentId, raptorPaddock)

        assertTrue(raptorPaddock !in mapPoolAdminRepository.poolMapIds(tournamentId))
    }

    @Test
    fun `removing a map not in the pool is rejected`() {
        val tournamentId = winterOfChampionsId()
        val created = adminMapService.create("Never Pooled Arena")

        assertFailsWith<NotFoundException> {
            adminMapService.removeFromPool(tournamentId, requireNotNull(created.id))
        }
    }

    @Test
    fun `removing a map with a recorded match on it is rejected, rather than surfacing a raw FK error`() {
        val tournamentId = summerOfLegendsId()
        val baskervilleManor = requireNotNull(gameMapRepository.findByName("Baskerville Manor")?.id)

        assertFailsWith<ConflictException> {
            adminMapService.removeFromPool(tournamentId, baskervilleManor)
        }
        assertTrue(baskervilleManor in mapPoolAdminRepository.poolMapIds(tournamentId), "the pool row must survive the rejected removal")
    }
}
