package com.umfl.tournament

import com.umfl.common.ConflictException
import com.umfl.common.RosterRuleException
import com.umfl.hero.HeroQueryRepository
import com.umfl.manager.ManagerRepository
import com.umfl.standings.StandingsQuery
import com.umfl.support.PostgresIntegrationTest
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.jdbc.core.simple.JdbcClient
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * The Roster Builder walkthrough, end to end against the database.
 *
 * Heroes are looked up by name: there is no slug any more, and the seed keys
 * every reference row on its natural name for exactly this reason.
 */
class RosterFlowIntegrationTest @Autowired constructor(
    private val tournamentService: TournamentService,
    private val tournamentRepository: TournamentRepository,
    private val entryRepository: TournamentEntryRepository,
    private val entryQuery: TournamentEntryQuery,
    private val managerRepository: ManagerRepository,
    private val heroQueryRepository: HeroQueryRepository,
    private val standingsQuery: StandingsQuery,
    private val jdbcClient: JdbcClient,
) : PostgresIntegrationTest() {

    private fun manager(handle: String) = assertNotNull(managerRepository.findByHandle(handle))

    private fun winterOfChampions() = assertNotNull(tournamentRepository.findByName("Winter of Champions"))

    private fun heroId(name: String): Long =
        jdbcClient.sql("select id from heroes where name = :name")
            .param("name", name)
            .query(Long::class.java)
            .single()

    private fun heroIds(vararg names: String): List<Long> = names.map(::heroId)

    @Test
    fun `registering is free and opens an empty draft holding the tournament's grant`() {
        val tournament = winterOfChampions()

        val snapshot = tournamentService.register(requireNotNull(tournament.id), manager("SherlockMain"))

        assertEquals(EntryStatus.DRAFT, snapshot.entry.status)
        assertTrue(snapshot.entry.slots.isEmpty())
        assertEquals(tournament.creditGrant, snapshot.entry.creditGrant)
        assertEquals(0, snapshot.budget.spent)
        assertEquals(10_000, snapshot.budget.remaining)
    }

    @Test
    fun `registering twice is rejected`() {
        val tournament = requireNotNull(winterOfChampions().id)
        tournamentService.register(tournament, manager("MythicMind"))

        assertFailsWith<ConflictException> {
            tournamentService.register(tournament, manager("MythicMind"))
        }
    }

    @Test
    fun `a full tournament is closed to new entries`() {
        val tournament = requireNotNull(winterOfChampions().id)
        tournamentService.register(tournament, manager("SherlockMain"))

        // Shrink capacity to what is already taken rather than registering 64
        // managers the seed does not have.
        val taken = entryRepository.countByTournamentId(tournament)
        jdbcClient.sql("update tournament set capacity = :c where id = :id")
            .param("c", taken)
            .param("id", tournament)
            .update()

        val error = assertFailsWith<ConflictException> {
            tournamentService.register(tournament, manager("MythicMind"))
        }
        assertTrue(error.message!!.contains("is full"), error.message!!)
    }

    @Test
    fun `a finished tournament is closed to new entries`() {
        val summer = assertNotNull(tournamentRepository.findByName("Summer of Legends"))

        val error = assertFailsWith<ConflictException> {
            tournamentService.register(requireNotNull(summer.id), manager("SherlockMain"))
        }
        assertTrue(error.message!!.contains("COMPLETED"), error.message!!)
    }

    @Test
    fun `an over-budget draft is saved but cannot be locked`() {
        val tournament = requireNotNull(winterOfChampions().id)
        val handle = "ArthurianLegend"
        tournamentService.register(tournament, manager(handle))

        // Sun Wukong 5300 + Medusa 5600 + King Arthur 4700 = 15,600 against a 10,000 grant.
        val premium = heroIds("Sun Wukong", "Medusa", "King Arthur")
        val overBudget = tournamentService.setSlots(tournament, manager(handle), premium)

        assertEquals(15_600, overBudget.budget.spent)
        assertEquals(-5_600, overBudget.budget.remaining)
        assertEquals(3, overBudget.entry.slots.size)

        val error = assertFailsWith<RosterRuleException> {
            tournamentService.lockRoster(tournament, manager(handle))
        }
        assertEquals(listOf(RosterRule.BUDGET_EXCEEDED), error.violations.map { it.rule })

        // Swapping to a legal trio unblocks the lock.
        val legal = heroIds("Alice", "Robin Hood", "Bigfoot")
        val fixed = tournamentService.setSlots(tournament, manager(handle), legal)
        assertEquals(9_400, fixed.budget.spent)
        assertEquals(600, fixed.budget.remaining)

        val locked = tournamentService.lockRoster(tournament, manager(handle))
        assertEquals(EntryStatus.LOCKED, locked.entry.status)
        assertNotNull(locked.entry.lockedAt)
    }

    @Test
    fun `the lobby's entry-status projection tracks registration and locking`() {
        val tournament = requireNotNull(winterOfChampions().id)
        val handle = "MythicMind"
        val managerId = requireNotNull(manager(handle).id)

        // Every seeded manager holds a locked Summer of Legends entry; Winter starts empty.
        assertEquals(null, entryQuery.statusesByTournament(managerId)[tournament])

        tournamentService.register(tournament, manager(handle))
        assertEquals(EntryStatus.DRAFT, entryQuery.statusesByTournament(managerId)[tournament])

        tournamentService.setSlots(tournament, manager(handle), heroIds("Alice", "Robin Hood", "Bigfoot"))
        tournamentService.lockRoster(tournament, manager(handle))
        val statuses = entryQuery.statusesByTournament(managerId)
        assertEquals(EntryStatus.LOCKED, statuses[tournament])
        assertTrue(statuses.size > 1, "the seeded Summer of Legends entry is still in the projection")
    }

    @Test
    fun `a locked roster is immutable and survives a reload`() {
        val tournament = requireNotNull(winterOfChampions().id)
        val handle = "NeonStrategist"
        tournamentService.register(tournament, manager(handle))

        val picks = heroIds("Sherlock Holmes", "Yennenga", "Sinbad")
        tournamentService.setSlots(tournament, manager(handle), picks)
        val locked = tournamentService.lockRoster(tournament, manager(handle))
        assertEquals(8_200, locked.budget.spent, "3400 + 2900 + 1900 at Winter prices")

        val error = assertFailsWith<RosterRuleException> {
            tournamentService.setSlots(tournament, manager(handle), picks.reversed())
        }
        assertEquals(listOf(RosterRule.ENTRY_LOCKED), error.violations.map { it.rule })

        val reloaded = assertNotNull(
            entryRepository.findByTournamentIdAndManagerId(tournament, requireNotNull(manager(handle).id))
        )
        assertEquals(EntryStatus.LOCKED, reloaded.status)
        assertEquals(picks, reloaded.heroIds, "slot order must survive the round trip")
    }

    @Test
    fun `duplicate picks are rejected`() {
        val tournament = requireNotNull(winterOfChampions().id)
        val handle = "SherlockMain"
        tournamentService.register(tournament, manager(handle))

        val alice = heroId("Alice")
        val error = assertFailsWith<RosterRuleException> {
            tournamentService.setSlots(tournament, manager(handle), listOf(alice, alice))
        }

        assertEquals(listOf(RosterRule.DUPLICATE_HERO), error.violations.map { it.rule })
    }

    @Test
    fun `a hero id that does not exist at all is rejected`() {
        val tournament = requireNotNull(winterOfChampions().id)
        val handle = "MythicMind"
        tournamentService.register(tournament, manager(handle))

        val error = assertFailsWith<RosterRuleException> {
            tournamentService.setSlots(tournament, manager(handle), listOf(999_999L))
        }

        assertEquals(listOf(RosterRule.UNKNOWN_HERO), error.violations.map { it.rule })
    }

    @Test
    fun `a real hero outside this tournament's pool is just as unknown`() {
        // Spring of Myths carries eight of the twelve heroes; Bigfoot is not one
        // of them, so this is UNKNOWN_HERO with an id that genuinely exists.
        val spring = assertNotNull(tournamentRepository.findByName("Spring of Myths"))
        val springId = requireNotNull(spring.id)
        val handle = "NeonStrategist"
        // Spring is SCHEDULED, so it takes no registrations yet — the entry is
        // created directly. Rosters still accept changes in that state.
        entryRepository.save(
            TournamentEntry(
                tournamentId = springId,
                managerId = requireNotNull(manager(handle).id),
                status = EntryStatus.DRAFT,
                creditGrant = spring.creditGrant,
            )
        )

        val bigfoot = heroId("Bigfoot")
        assertTrue(
            heroQueryRepository.findByIds(springId, listOf(bigfoot)).isEmpty(),
            "precondition: Bigfoot is outside Spring's pool",
        )

        val error = assertFailsWith<RosterRuleException> {
            tournamentService.setSlots(springId, manager(handle), listOf(bigfoot))
        }

        assertEquals(listOf(RosterRule.UNKNOWN_HERO), error.violations.map { it.rule })
        assertTrue(error.message!!.contains("Spring of Myths"), error.message!!)
    }

    @Test
    fun `re-pricing a hero re-prices an unlocked roster`() {
        val tournament = requireNotNull(winterOfChampions().id)
        val handle = "MythicMind"
        tournamentService.register(tournament, manager(handle))

        val picks = heroIds("Alice", "Robin Hood", "Bigfoot")
        assertEquals(9_400, tournamentService.setSlots(tournament, manager(handle), picks).budget.spent)

        // An admin retunes Bigfoot from 2,100 to 3,000. Nothing was snapshotted,
        // so the draft re-prices itself and is now over budget.
        jdbcClient
            .sql("update tournament_hero set cost = 3000 where tournament_id = :t and hero_id = :h")
            .param("t", tournament)
            .param("h", heroId("Bigfoot"))
            .update()

        val reloaded = assertNotNull(tournamentService.findMyEntry(tournament, manager(handle)))
        assertEquals(10_300, reloaded.budget.spent)
        assertEquals(-300, reloaded.budget.remaining)
        assertEquals(listOf(4_100, 3_200, 3_000), reloaded.heroes.map { it.cost })

        val error = assertFailsWith<RosterRuleException> {
            tournamentService.lockRoster(tournament, manager(handle))
        }
        assertEquals(listOf(RosterRule.BUDGET_EXCEEDED), error.violations.map { it.rule })
    }

    @Test
    fun `a hero pulled from the pool after locking is kept at cost 0, not dropped`() {
        val tournament = requireNotNull(winterOfChampions().id)
        val handle = "SherlockMain"
        tournamentService.register(tournament, manager(handle))

        val picks = heroIds("Alice", "Robin Hood", "Bigfoot")
        tournamentService.setSlots(tournament, manager(handle), picks)
        tournamentService.lockRoster(tournament, manager(handle))

        // Bigfoot leaves Winter's pool entirely (UMFL-06's premise: a hero
        // still on a locked roster is later pulled from tournament_hero).
        jdbcClient
            .sql("delete from tournament_hero where tournament_id = :t and hero_id = :h")
            .param("t", tournament)
            .param("h", heroId("Bigfoot"))
            .update()

        val reloaded = assertNotNull(tournamentService.findMyEntry(tournament, manager(handle)))
        assertEquals(3, reloaded.heroes.size, "the slot survives, priced at 0, not silently dropped")
        assertEquals(listOf(4_100, 3_200, 0), reloaded.heroes.map { it.cost })
        assertEquals(7_300, reloaded.budget.spent)

        val entryRoster = assertNotNull(
            standingsQuery.rosters(tournament).singleOrNull { it.handle == handle }
        )
        assertEquals(reloaded.heroes.size, entryRoster.heroes.size, "same roster length on both reads")
        assertEquals(reloaded.budget.spent, entryRoster.spent, "same spend on both reads")
    }

    @Test
    fun `the hero pool is scoped to the tournament, priced and sortable`() {
        val winter = requireNotNull(winterOfChampions().id)

        val byCost = heroQueryRepository.findByTournament(winter)
        assertEquals(12, byCost.size)
        assertEquals("Medusa", byCost.first().name, "the most expensive Winter pick")
        assertEquals(5_600, byCost.first().cost)
        assertEquals(byCost.map { it.cost }.sortedDescending(), byCost.map { it.cost })

        val searched = heroQueryRepository.findByTournament(winter, com.umfl.hero.HeroFilter(search = "ho"))
        assertEquals(listOf("Robin Hood", "Sherlock Holmes"), searched.map { it.name }.sorted())

        val wildcardSearch = heroQueryRepository.findByTournament(winter, com.umfl.hero.HeroFilter(search = "_"))
        assertEquals(emptyList<String>(), wildcardSearch.map { it.name }, "_ is a literal, not a match-any-char wildcard")

        val percentSearch = heroQueryRepository.findByTournament(winter, com.umfl.hero.HeroFilter(search = "%"))
        assertEquals(emptyList<String>(), percentSearch.map { it.name }, "% is a literal, not a match-all wildcard")
    }
}
