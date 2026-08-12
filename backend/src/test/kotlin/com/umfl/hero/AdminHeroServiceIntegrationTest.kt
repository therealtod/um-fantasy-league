package com.umfl.hero

import com.umfl.common.ConflictException
import com.umfl.common.NotFoundException
import com.umfl.manager.ManagerRepository
import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.TournamentRepository
import com.umfl.tournament.TournamentService
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class AdminHeroServiceIntegrationTest @Autowired constructor(
    private val adminHeroService: AdminHeroService,
    private val heroRepository: HeroRepository,
    private val heroQueryRepository: HeroQueryRepository,
    private val tournamentService: TournamentService,
    private val tournamentRepository: TournamentRepository,
    private val managerRepository: ManagerRepository,
) : PostgresIntegrationTest() {

    private fun winterOfChampionsId() = requireNotNull(tournamentRepository.findByName("Winter of Champions")?.id)

    @Test
    fun `creates a new hero`() {
        val created = adminHeroService.create("Achilles' Shield", null)

        assertNotNull(created.id)
        assertNotNull(heroRepository.findByName("Achilles' Shield"))
    }

    @Test
    fun `creating a hero with a name already in use is rejected`() {
        assertFailsWith<ConflictException> { adminHeroService.create("Alice", null) }
    }

    @Test
    fun `updates an existing hero's name and artwork`() {
        val created = adminHeroService.create("Placeholder Hero", null)

        val updated = adminHeroService.update(requireNotNull(created.id), "Renamed Hero", "https://example.test/art.png")

        assertEquals("Renamed Hero", updated.name)
        assertEquals("https://example.test/art.png", updated.imageUrl)
    }

    @Test
    fun `adds a brand-new hero to a tournament's pool at the given cost`() {
        val tournamentId = winterOfChampionsId()
        val hero = adminHeroService.create("Fresh Recruit", null)
        val heroId = requireNotNull(hero.id)

        val view = adminHeroService.setPoolCost(tournamentId, heroId, 3_300)

        assertEquals(3_300, view.cost)
        assertEquals(listOf(3_300), heroQueryRepository.findByIds(tournamentId, listOf(heroId)).map { it.cost })
    }

    @Test
    fun `re-pricing through the admin path re-prices an unlocked roster, the same as the raw SQL path does`() {
        val tournamentId = winterOfChampionsId()
        val bigfootId = heroQueryRepository.findByTournament(tournamentId).single { it.name == "Bigfoot" }.id
        val before = heroQueryRepository.findByIds(tournamentId, listOf(bigfootId)).single().cost
        assertEquals(2_100, before)

        val manager = requireNotNull(managerRepository.findByHandle("MythicMind"))
        tournamentService.register(tournamentId, manager)
        val drafted = tournamentService.setSlots(tournamentId, manager, listOf(bigfootId))
        assertEquals(2_100, drafted.budget.spent)

        val repriced = adminHeroService.setPoolCost(tournamentId, bigfootId, 3_000)
        assertEquals(3_000, repriced.cost)

        val reloaded = requireNotNull(tournamentService.findMyEntry(tournamentId, manager))
        assertEquals(3_000, reloaded.budget.spent, "nothing was snapshotted, so the draft re-prices itself")
    }

    @Test
    fun `addBatchToPool adds several brand-new heroes in one call`() {
        val tournamentId = winterOfChampionsId()
        val heroA = requireNotNull(adminHeroService.create("Batch Hero A", null).id)
        val heroB = requireNotNull(adminHeroService.create("Batch Hero B", null).id)

        val result = adminHeroService.addBatchToPool(
            tournamentId,
            listOf(HeroPoolEntryInput(heroA, 1_200), HeroPoolEntryInput(heroB, 2_400)),
        )

        assertEquals(setOf(1_200, 2_400), result.map { it.cost }.toSet())
        val pool = heroQueryRepository.findByIds(tournamentId, listOf(heroA, heroB)).associate { it.id to it.cost }
        assertEquals(1_200, pool[heroA])
        assertEquals(2_400, pool[heroB])
    }

    @Test
    fun `addBatchToPool re-prices a hero already in the pool alongside adding a new one`() {
        val tournamentId = winterOfChampionsId()
        val bigfootId = heroQueryRepository.findByTournament(tournamentId).single { it.name == "Bigfoot" }.id
        val newHero = requireNotNull(adminHeroService.create("Batch Hero C", null).id)

        adminHeroService.addBatchToPool(
            tournamentId,
            listOf(HeroPoolEntryInput(bigfootId, 3_500), HeroPoolEntryInput(newHero, 1_000)),
        )

        val pool = heroQueryRepository.findByIds(tournamentId, listOf(bigfootId, newHero)).associate { it.id to it.cost }
        assertEquals(3_500, pool[bigfootId])
        assertEquals(1_000, pool[newHero])
    }

    @Test
    fun `addBatchToPool rejects an unknown hero id and writes nothing from the batch`() {
        val tournamentId = winterOfChampionsId()
        val newHero = requireNotNull(adminHeroService.create("Batch Hero D", null).id)
        val unknownId = 9_999_999L

        assertFailsWith<NotFoundException> {
            adminHeroService.addBatchToPool(
                tournamentId,
                listOf(HeroPoolEntryInput(newHero, 1_500), HeroPoolEntryInput(unknownId, 1_500)),
            )
        }

        assertTrue(
            heroQueryRepository.findByIds(tournamentId, listOf(newHero)).isEmpty(),
            "the whole batch rolls back, including the valid entry",
        )
    }

    @Test
    fun `pool lists a tournament's hero pool, admin-scoped`() {
        val tournamentId = winterOfChampionsId()

        val pool = adminHeroService.pool(tournamentId)

        assertTrue(pool.any { it.name == "Bigfoot" })
        assertEquals(pool, heroQueryRepository.findByTournament(tournamentId))
    }

    @Test
    fun `removes a hero from a tournament's pool`() {
        val tournamentId = winterOfChampionsId()
        val bigfootId = heroQueryRepository.findByTournament(tournamentId).single { it.name == "Bigfoot" }.id

        adminHeroService.removeFromPool(tournamentId, bigfootId)

        assertTrue(heroQueryRepository.findByTournament(tournamentId).none { it.id == bigfootId })
    }

    @Test
    fun `removing a hero not in the pool is rejected`() {
        val tournamentId = winterOfChampionsId()
        val hero = adminHeroService.create("Never Pooled", null)

        assertFailsWith<NotFoundException> {
            adminHeroService.removeFromPool(tournamentId, requireNotNull(hero.id))
        }
    }

    @Test
    fun `removing a hero from the pool re-prices an unlocked roster holding it to 0, same as re-pricing does`() {
        val tournamentId = winterOfChampionsId()
        val bigfootId = heroQueryRepository.findByTournament(tournamentId).single { it.name == "Bigfoot" }.id
        val manager = requireNotNull(managerRepository.findByHandle("MythicMind"))
        tournamentService.register(tournamentId, manager)
        val drafted = tournamentService.setSlots(tournamentId, manager, listOf(bigfootId))
        assertEquals(2_100, drafted.budget.spent)

        adminHeroService.removeFromPool(tournamentId, bigfootId)

        val reloaded = requireNotNull(tournamentService.findMyEntry(tournamentId, manager))
        assertEquals(0, reloaded.budget.spent, "the slot still holds the hero, but it costs nothing once out of the pool")
    }
}
