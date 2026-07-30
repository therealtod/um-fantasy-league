package com.umfl.hero

import com.umfl.common.ConflictException
import com.umfl.manager.ManagerRepository
import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.TournamentRepository
import com.umfl.tournament.TournamentService
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull

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
}
