package com.umfl.matchimport

import com.umfl.map.GameMapRepository
import com.umfl.map.MapPoolAdminRepository
import com.umfl.match.BanType
import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.TournamentRepository
import org.junit.jupiter.api.Test
import org.mockito.Mockito.`when`
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.test.context.bean.override.mockito.MockitoBean
import tools.jackson.databind.ObjectMapper
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Exercises the importer against the real catalogue and the seeded board pools,
 * with only the network boundary stubbed.
 *
 * The fixture in `resources/matchimport/sample-match.json` is a genuine capture
 * of a live scrape, not a hand-written sample — it is the actual shape the
 * sidecar returns, including a negative health value and a ten-hero draft. If
 * the source site's markup drifts, re-capturing it here is what surfaces the
 * change.
 */
class MatchImportServiceTest @Autowired constructor(
    private val service: MatchImportService,
    private val mapPool: MapPoolAdminRepository,
    private val gameMaps: GameMapRepository,
    private val tournaments: TournamentRepository,
    private val objectMapper: ObjectMapper,
) : PostgresIntegrationTest() {

    @MockitoBean
    private lateinit var scraperClient: ScraperClient

    private val sourceUrl =
        "https://www.tabletopleague.com/o/umleague/summer-of-legends-2026/matches/92569156-bf1b-4be1-93ad-f1bdffbdbc2e"

    private fun tournamentId(name: String): Long =
        requireNotNull(tournaments.findAll().first { it.name == name }.id)

    private fun mapIdOf(name: String): Long =
        requireNotNull(gameMaps.findByName(name)?.id) { "seed catalogue is missing board $name" }

    private fun stubScrape(): ScrapedMatch {
        val json = requireNotNull(javaClass.getResourceAsStream("/matchimport/sample-match.json"))
            .readAllBytes().decodeToString()
        val scraped = objectMapper.readValue(json, ScrapedMatch::class.java)
        `when`(scraperClient.validateSourceUrl(sourceUrl)).thenReturn(null)
        `when`(scraperClient.scrapeMatch(sourceUrl)).thenReturn(scraped)
        return scraped
    }

    /** Sanity check on the fixture itself, so a bad capture fails loudly rather than skewing the rest. */
    @Test
    fun `the captured fixture deserializes into the fields the importer reads`() {
        val scraped = stubScrape()
        assertEquals("The Wayward Sisters", scraped.roundName)
        assertEquals("BO3", scraped.seriesFormat)
        assertEquals("17 Aug 2026, 22:00 CEST", scraped.playedAtRaw)
        assertEquals("mystic_owl", scraped.sideA?.playerLabel)
        assertEquals(3, scraped.games.size)
        assertEquals(6, scraped.preBans.size)
        assertEquals(-2, scraped.games[2].sideA?.health)
    }

    @Test
    fun `resolves every hero and board when the pool has them all`() {
        stubScrape()
        val tournamentId = tournamentId("Summer of Legends")
        // The seed's Summer pool has only three boards, none of them this match's.
        listOf("Technodrome", "Raptor Paddock", "Navy Pier").forEach { name ->
            mapPool.addToPool(tournamentId, mapIdOf(name))
        }

        val preview = service.preview(tournamentId, sourceUrl)

        assertEquals(emptyList(), preview.unresolved)
        assertEquals(3, preview.games.size)
        assertTrue(preview.games.all { it.mapId != null })
        assertTrue(preview.games.flatMap { it.participants }.all { it.heroId != null })
        assertTrue(preview.bans.all { it.heroId != null })
        assertTrue(preview.participants.all { p -> p.draftedHeroIds.size == 3 })
    }

    @Test
    fun `maps the series faithfully`() {
        stubScrape()
        val tournamentId = tournamentId("Summer of Legends")
        listOf("Technodrome", "Raptor Paddock", "Navy Pier").forEach { mapPool.addToPool(tournamentId, mapIdOf(it)) }

        val preview = service.preview(tournamentId, sourceUrl)

        assertEquals(listOf("mystic_owl", "immortal"), preview.participants.map { it.playerLabel })
        assertEquals(listOf(1, 2, 3), preview.games.map { it.gameNumber })
        assertEquals(listOf("Technodrome", "Raptor Paddock", "Navy Pier"), preview.games.map { it.mapName })

        // Game 1: side A wins on 5, side B is defeated on exactly 0.
        val game1 = preview.games[0]
        assertEquals(listOf(true, false), game1.participants.map { it.isWinner })
        assertEquals(listOf(5, 0), game1.participants.map { it.healthRemaining })

        // Game 3: the loser finished below zero on an overkill hit — the value
        // MatchResultPolicy.LOSER_HAS_POSITIVE_HEALTH exists to allow.
        assertEquals(-2, preview.games[2].participants[0].healthRemaining)
        assertEquals(listOf(false, true), preview.games[2].participants.map { it.isWinner })

        assertEquals("The Wayward Sisters", preview.roundName)
        assertEquals("BO3", preview.seriesFormat)
        // 22:00 CEST is 20:00Z.
        assertEquals("2026-08-17T20:00:00Z", preview.playedAt.toString())
    }

    /** Both sides' typed bans plus the shared pre-ban pool land in one flat list, as `hero_bans` stores them. */
    @Test
    fun `flattens both sides' bans and the pre-ban pool`() {
        stubScrape()
        val tournamentId = tournamentId("Summer of Legends")

        val preview = service.preview(tournamentId, sourceUrl)

        assertEquals(10, preview.bans.size)
        assertEquals(2, preview.bans.count { it.banType == BanType.OPPONENT_BAN })
        assertEquals(2, preview.bans.count { it.banType == BanType.SELF_BAN })
        assertEquals(6, preview.bans.count { it.banType == BanType.PRE_BAN })
        assertEquals(
            setOf("Alice", "John Henry"),
            preview.bans.filter { it.banType == BanType.OPPONENT_BAN }.map { it.heroName }.toSet(),
        )
        // Picks and bans stay disjoint, or BANNED_HERO_DRAFTED would fire on save.
        val drafted = preview.participants.flatMap { it.draftedHeroIds }.toSet()
        assertTrue(preview.bans.mapNotNull { it.heroId }.none { it in drafted })
    }

    /**
     * Flat list, but not side-blind: the source files a typed ban under the side
     * that owned the hero, and `hero_bans.side` now keeps it. A pre-ban is struck
     * before sides are assigned and carries none, which is also what
     * `MatchRule.BAN_SIDE_INVALID` insists on.
     */
    @Test
    fun `keeps the side a typed ban was struck from, and leaves a pre-ban unsided`() {
        stubScrape()
        val tournamentId = tournamentId("Summer of Legends")

        val preview = service.preview(tournamentId, sourceUrl)

        assertTrue(
            preview.bans.filter { it.banType == BanType.PRE_BAN }.all { it.side == null },
            "a pre-ban belongs to neither side",
        )
        val sideByHero: Map<String?, Int?> = preview.bans
            .filterNot { it.banType == BanType.PRE_BAN }
            .associate { it.heroName to it.side }
        assertEquals(
            mapOf<String?, Int?>("Alice" to 0, "Daredevil" to 0, "John Henry" to 1, "Dr. Jill Trent" to 1),
            sideByHero,
            "side A's two bans came out of side A's draft, side B's out of side B's",
        )
    }

    /**
     * The board exists in `game_maps` but not in this tournament's pool. It is
     * reported, not invented — `match_games`'s composite FK onto `tournament_maps`
     * means recording a game on it would fail at the database.
     */
    @Test
    fun `reports a board that is missing from this tournament's pool`() {
        stubScrape()
        // The seed's Summer pool has Raptor Paddock but neither Technodrome nor Navy Pier.
        val tournamentId = tournamentId("Summer of Legends")

        val preview = service.preview(tournamentId, sourceUrl)

        val missing = preview.unresolved.filter { it.reason == UnresolvedReason.MAP_NOT_IN_POOL }
        assertEquals(setOf("Technodrome", "Navy Pier"), missing.map { it.sourceName }.toSet())
        assertTrue(missing.all { it.kind == UnresolvedKind.MAP })
        // The id is carried so the client can offer to add it to the pool in one click.
        assertTrue(missing.all { it.mapId != null })
        // The unresolved games have no map, but the resolvable one still does.
        assertNull(preview.games[0].mapId)
        assertNotNull(preview.games[1].mapId)
        // Heroes are unaffected: they reference `heroes(id)`, never `tournament_heroes`.
        assertTrue(preview.unresolved.none { it.kind == UnresolvedKind.HERO })
    }

    /** A hero the catalogue has never heard of is named once, not once per appearance. */
    @Test
    fun `reports an unknown hero exactly once`() {
        val scraped = stubScrape()
        val ghost = "Nonexistent Hero"
        val tampered = scraped.copy(
            sideA = scraped.sideA!!.copy(picks = listOf(ghost) + scraped.sideA!!.picks.drop(1)),
            games = scraped.games.mapIndexed { i, g ->
                if (i == 0) g.copy(sideA = g.sideA!!.copy(heroName = ghost)) else g
            },
        )
        `when`(scraperClient.scrapeMatch(sourceUrl)).thenReturn(tampered)

        val preview = service.preview(tournamentId("Summer of Legends"), sourceUrl)

        val unknownHeroes = preview.unresolved.filter { it.reason == UnresolvedReason.UNKNOWN_HERO }
        assertEquals(1, unknownHeroes.size)
        assertEquals(ghost, unknownHeroes.single().sourceName)
        assertNull(preview.games[0].participants[0].heroId)
        // The name is still carried so the admin can see what failed.
        assertEquals(ghost, preview.games[0].participants[0].heroName)
    }

    /** A timezone the parser can't resolve costs the timestamp, never the import. */
    @Test
    fun `leaves playedAt null when the timestamp is unparseable`() {
        val scraped = stubScrape()
        `when`(scraperClient.scrapeMatch(sourceUrl))
            .thenReturn(scraped.copy(playedAtRaw = "sometime on Tuesday"))

        val preview = service.preview(tournamentId("Summer of Legends"), sourceUrl)

        assertNull(preview.playedAt)
        assertEquals("sometime on Tuesday", preview.playedAtRaw)
        assertEquals(3, preview.games.size)
    }

    @Test
    fun `reports no duplicate when this url has not been imported`() {
        stubScrape()
        assertNull(service.preview(tournamentId("Summer of Legends"), sourceUrl).alreadyImportedMatchId)
    }
}
