package com.umfl.matchimport

import com.umfl.manager.ManagerRepository
import com.umfl.map.GameMapRepository
import com.umfl.map.MapPoolAdminRepository
import com.umfl.support.PostgresIntegrationTest
import com.umfl.tournament.TournamentRepository
import org.hamcrest.Matchers.containsString
import org.junit.jupiter.api.Test
import org.mockito.Mockito.`when`
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.boot.webmvc.test.autoconfigure.AutoConfigureMockMvc
import org.springframework.http.MediaType
import org.springframework.test.context.bean.override.mockito.MockitoBean
import org.springframework.test.web.servlet.MockMvc
import org.springframework.test.web.servlet.post
import tools.jackson.databind.ObjectMapper
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * The round trip that matters: a preview, submitted unchanged to the ordinary
 * record endpoint, is accepted.
 *
 * The importer deliberately knows nothing about `MatchResultPolicy` — it
 * resolves names and stops. That is only safe if the draft it produces is
 * actually recordable, and the two are far enough apart in the codebase that
 * nothing else would catch a drift between them. This test is the seam: it
 * fails if the importer ever starts emitting something the policy rejects
 * (a hero played but not drafted, a ban colliding with a pick, non-sequential
 * game numbers, a loser left on positive health).
 */
@AutoConfigureMockMvc
class MatchImportEndpointTest @Autowired constructor(
    private val mockMvc: MockMvc,
    private val managers: ManagerRepository,
    private val tournaments: TournamentRepository,
    private val gameMaps: GameMapRepository,
    private val mapPool: MapPoolAdminRepository,
    private val objectMapper: ObjectMapper,
) : PostgresIntegrationTest() {

    @MockitoBean
    private lateinit var scraperClient: ScraperClient

    private val sourceUrl =
        "https://www.tabletopleague.com/o/umleague/summer-of-legends-2026/matches/92569156-bf1b-4be1-93ad-f1bdffbdbc2e"

    private val adminId: String
        get() = requireNotNull(managers.findByHandle("NeonStrategist")).id.toString()

    private val tournamentId: Long
        get() = requireNotNull(tournaments.findAll().first { it.name == "Summer of Legends" }.id)

    private fun stubScrape() {
        val json = requireNotNull(javaClass.getResourceAsStream("/matchimport/sample-match.json"))
            .readAllBytes().decodeToString()
        `when`(scraperClient.validateSourceUrl(sourceUrl)).thenReturn(null)
        `when`(scraperClient.scrapeMatch(sourceUrl))
            .thenReturn(objectMapper.readValue(json, ScrapedMatch::class.java))
    }

    /** Every board in the fixture, so the preview comes back fully resolved. */
    private fun stockPool(tournamentId: Long) {
        listOf("Technodrome", "Raptor Paddock", "Navy Pier").forEach {
            mapPool.addToPool(tournamentId, requireNotNull(gameMaps.findByName(it)?.id))
        }
    }

    @Test
    fun `a fully resolved preview is accepted verbatim by the record endpoint`() {
        stubScrape()
        val tournamentId = tournamentId
        stockPool(tournamentId)

        val previewJson = mockMvc.post("/api/admin/tournaments/$tournamentId/matches/import") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "$sourceUrl"}"""
        }.andExpect { status { isOk() } }.andReturn().response.contentAsString

        val preview = objectMapper.readTree(previewJson)
        assertTrue(preview["unresolved"].isEmpty, "expected a fully resolved preview: $previewJson")

        // Build exactly what the client builds: the preview, plus the round
        // number only a human can supply.
        val recordBody = objectMapper.createObjectNode().apply {
            put("round", 1)
            put("playedAt", preview["playedAt"].asString())
            put("externalLink", preview["sourceUrl"].asString())
            set("participants", preview["participants"].deepCopy())
            set("games", preview["games"].deepCopy())
            set("bans", preview["bans"].deepCopy())
        }

        val recordedJson = mockMvc.post("/api/admin/tournaments/$tournamentId/matches") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = recordBody.toString()
        }.andExpect {
            status { isCreated() }
            jsonPath("$.round") { value(1) }
            jsonPath("$.games.length()") { value(3) }
            jsonPath("$.bans.length()") { value(10) }
            jsonPath("$.externalLink") { value(sourceUrl) }
        }.andReturn().response.contentAsString

        // The side survives the whole round trip: the source grouped these under
        // the side that owned the hero, the preview kept it, and `hero_bans.side`
        // stored it. Asserted off the parsed body rather than with a jsonPath
        // filter, which returns a length per match rather than a match count.
        val recordedBans = objectMapper.readTree(recordedJson)["bans"]
        assertEquals(
            listOf(2, 2),
            listOf(0, 1).map { side -> recordedBans.count { it["side"]?.asInt() == side } },
            "each side's two typed bans came back attributed to it",
        )
        assertTrue(
            recordedBans.filter { it["banType"].asString() == "PRE_BAN" }.all { it["side"] == null },
            "a pre-ban precedes side assignment, so Jackson omits the null side entirely",
        )
    }

    /**
     * Re-importing a URL already recorded names the existing match instead of
     * silently duplicating it — and recording a second copy is refused outright
     * by `uq_tournament_match_external_link`, so the preview's warning and the
     * write path agree rather than the admin discovering the conflict on save.
     */
    @Test
    fun `a second import of the same url reports the existing match and cannot be recorded twice`() {
        stubScrape()
        val tournamentId = tournamentId
        stockPool(tournamentId)

        val first = mockMvc.post("/api/admin/tournaments/$tournamentId/matches/import") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "$sourceUrl"}"""
        }.andReturn().response.contentAsString
        val preview = objectMapper.readTree(first)
        // Jackson runs with non_null inclusion, so "no duplicate" is an absent
        // field rather than a null one — the same contract the frontend types treat
        // as optional.
        assertNull(preview["alreadyImportedMatchId"])

        val recordBody = objectMapper.createObjectNode().apply {
            put("round", 1)
            put("playedAt", preview["playedAt"].asString())
            put("externalLink", sourceUrl)
            set("participants", preview["participants"].deepCopy())
            set("games", preview["games"].deepCopy())
            set("bans", preview["bans"].deepCopy())
        }
        val recorded = mockMvc.post("/api/admin/tournaments/$tournamentId/matches") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = recordBody.toString()
        }.andReturn().response.contentAsString
        val matchId = objectMapper.readTree(recorded)["matchId"].asLong()

        mockMvc.post("/api/admin/tournaments/$tournamentId/matches/import") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "$sourceUrl"}"""
        }.andExpect {
            status { isOk() }
            jsonPath("$.alreadyImportedMatchId") { value(matchId) }
        }

        // The link is what makes the duplicate detectable, so posting the same
        // draft again is a 409 that names the match to correct — not a second
        // row quietly double-counting every point this match scores.
        mockMvc.post("/api/admin/tournaments/$tournamentId/matches") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = recordBody.toString()
        }.andExpect {
            status { isConflict() }
            jsonPath("$.detail") { value(containsString(matchId.toString())) }
        }
    }

    /** The link is the duplicate check, so it is required rather than optional. */
    @Test
    fun `recording a match without an external link is rejected`() {
        stockPool(tournamentId)
        stubScrape()

        val previewJson = mockMvc.post("/api/admin/tournaments/$tournamentId/matches/import") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "$sourceUrl"}"""
        }.andReturn().response.contentAsString
        val preview = objectMapper.readTree(previewJson)

        val body = objectMapper.createObjectNode().apply {
            put("round", 1)
            put("playedAt", preview["playedAt"].asString())
            put("externalLink", "   ")
            set("participants", preview["participants"].deepCopy())
            set("games", preview["games"].deepCopy())
            set("bans", preview["bans"].deepCopy())
        }

        mockMvc.post("/api/admin/tournaments/$tournamentId/matches") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = body.toString()
        }.andExpect { status { isBadRequest() } }
    }

    @Test
    fun `a non-admin manager cannot import`() {
        val nonAdmin = requireNotNull(managers.findByHandle("SherlockMain")).id.toString()
        mockMvc.post("/api/admin/tournaments/$tournamentId/matches/import") {
            header("X-Manager-Id", nonAdmin)
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "$sourceUrl"}"""
        }.andExpect { status { isForbidden() } }
    }

    @Test
    fun `an anonymous request cannot import`() {
        mockMvc.post("/api/admin/tournaments/$tournamentId/matches/import") {
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "$sourceUrl"}"""
        }.andExpect { status { isUnauthorized() } }
    }

    /** A URL the client shouldn't have sent is a clean 409, not a scrape attempt. */
    @Test
    fun `a url that is not a match page is rejected without scraping`() {
        `when`(scraperClient.validateSourceUrl("https://example.com/nope"))
            .thenReturn("That is not a match page.")

        mockMvc.post("/api/admin/tournaments/$tournamentId/matches/import") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "https://example.com/nope"}"""
        }.andExpect {
            status { isConflict() }
            content { contentType(MediaType.APPLICATION_PROBLEM_JSON) }
        }
    }

    @Test
    fun `a blank source url is a 400`() {
        mockMvc.post("/api/admin/tournaments/$tournamentId/matches/import") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "  "}"""
        }.andExpect { status { isBadRequest() } }
    }

    /** The sidecar being down is a 503 the admin can act on, not a 500. */
    @Test
    fun `an unreachable scraper surfaces as 503`() {
        `when`(scraperClient.validateSourceUrl(sourceUrl)).thenReturn(null)
        `when`(scraperClient.scrapeMatch(sourceUrl))
            .thenThrow(com.umfl.common.ServiceUnavailableException("scraper is not reachable"))

        mockMvc.post("/api/admin/tournaments/$tournamentId/matches/import") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "$sourceUrl"}"""
        }.andExpect {
            status { isServiceUnavailable() }
            content { contentType(MediaType.APPLICATION_PROBLEM_JSON) }
        }
    }

    @Test
    fun `importing against a tournament that does not exist is a 404`() {
        stubScrape()
        mockMvc.post("/api/admin/tournaments/999999/matches/import") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "$sourceUrl"}"""
        }.andExpect { status { isNotFound() } }
    }

    @Test
    fun `the preview names the round and format the source used`() {
        stubScrape()
        stockPool(tournamentId)
        val body = mockMvc.post("/api/admin/tournaments/$tournamentId/matches/import") {
            header("X-Manager-Id", adminId)
            contentType = MediaType.APPLICATION_JSON
            content = """{"sourceUrl": "$sourceUrl"}"""
        }.andReturn().response.contentAsString
        val preview = objectMapper.readTree(body)
        assertEquals("The Wayward Sisters", preview["roundName"].asString())
        assertEquals("BO3", preview["seriesFormat"].asString())
    }
}
