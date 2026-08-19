package com.umfl.matchimport

import com.umfl.common.ServiceUnavailableException
import org.junit.jupiter.api.Test
import java.net.ServerSocket
import java.time.Duration
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * No Spring context and no Docker: [ScraperClient] builds its own `RestClient`,
 * so it can be exercised directly. The unreachable-sidecar case points at a port
 * that was just closed, which is the real failure an admin hits when they forget
 * to start the scraper.
 */
class ScraperClientTest {

    private fun clientFor(baseUrl: String) = ScraperClient(
        ScraperProperties(
            baseUrl = baseUrl,
            timeout = Duration.ofSeconds(2),
            connectTimeout = Duration.ofMillis(500),
        )
    )

    /** A closed port must read as "start the scraper", not as an opaque 500. */
    @Test
    fun `an unreachable sidecar surfaces as ServiceUnavailable naming the base url`() {
        val port = ServerSocket(0).use { it.localPort } // bound, then immediately released
        val client = clientFor("http://127.0.0.1:$port")

        val ex = assertFailsWith<ServiceUnavailableException> {
            client.scrapeMatch("https://www.tabletopleague.com/o/o/c/matches/abcdef12")
        }
        assertTrue(ex.message!!.contains("127.0.0.1:$port"), "message should name the address: ${ex.message}")
        assertTrue(ex.message!!.contains("npm run serve"), "message should say how to start it: ${ex.message}")
    }

    // --- URL validation -----------------------------------------------------
    // Enforced here as well as in the sidecar. This is the copy that sees a URL
    // typed by a human, so it is the one whose message has to be useful.

    private val client = clientFor("http://localhost:3000")

    @Test
    fun `accepts a real match detail url`() {
        assertNull(
            client.validateSourceUrl(
                "https://www.tabletopleague.com/o/umleague/summer-of-legends-2026/matches/92569156-bf1b-4be1-93ad-f1bdffbdbc2e"
            )
        )
    }

    @Test
    fun `accepts a url with surrounding whitespace`() {
        assertNull(
            client.validateSourceUrl(
                "  https://www.tabletopleague.com/o/umleague/summer-of-legends-2026/matches/92569156-bf1b-4be1-93ad-f1bdffbdbc2e  "
            )
        )
    }

    @Test
    fun `rejects another host`() {
        assertNotNull(client.validateSourceUrl("https://example.com/o/a/b/matches/abcdef12"))
    }

    @Test
    fun `rejects plain http`() {
        assertNotNull(client.validateSourceUrl("http://www.tabletopleague.com/o/a/b/matches/abcdef12"))
    }

    /** The competition list page is not a match page — importing it would scrape nothing. */
    @Test
    fun `rejects a competition page`() {
        assertNotNull(client.validateSourceUrl("https://www.tabletopleague.com/o/umleague/summer-of-legends-2026/matches"))
    }

    /** The site's own API is off-limits, per the scraper's robots.txt commitment. */
    @Test
    fun `rejects the site's api routes`() {
        assertNotNull(client.validateSourceUrl("https://www.tabletopleague.com/api/matches/abcdef12"))
    }

    @Test
    fun `rejects junk`() {
        assertNotNull(client.validateSourceUrl(""))
        assertNotNull(client.validateSourceUrl("not a url"))
        assertNotNull(client.validateSourceUrl("ftp://www.tabletopleague.com/o/a/b/matches/abcdef12"))
    }

    @Test
    fun `names the allowed host in the rejection message`() {
        val message = client.validateSourceUrl("https://example.com/o/a/b/matches/abcdef12")
        assertEquals(
            "Only www.tabletopleague.com match pages can be imported.",
            message,
        )
    }
}
