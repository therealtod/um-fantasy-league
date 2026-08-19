package com.umfl.matchimport

import org.junit.jupiter.api.Test
import java.time.Instant
import kotlin.test.assertEquals
import kotlin.test.assertNull

class ScrapedTimestampsTest {

    /** The detail page's real shape, captured from a live scrape. */
    @Test
    fun `parses the detail page's timezone-qualified timestamp`() {
        // 22:00 CEST is UTC+2, so 20:00Z.
        assertEquals(Instant.parse("2026-08-17T20:00:00Z"), ScrapedTimestamps.parse("17 Aug 2026, 22:00 CEST"))
    }

    @Test
    fun `parses a winter CET timestamp at the other offset`() {
        assertEquals(Instant.parse("2026-01-17T21:00:00Z"), ScrapedTimestamps.parse("17 Jan 2026, 22:00 CET"))
    }

    @Test
    fun `parses UTC`() {
        assertEquals(Instant.parse("2026-08-17T22:00:00Z"), ScrapedTimestamps.parse("17 Aug 2026, 22:00 UTC"))
    }

    /**
     * A local time with no zone is not a point on the timeline. Returning null
     * hands the field to the admin rather than filing the match at a guessed hour.
     */
    @Test
    fun `returns null for the list page's zoneless timestamp`() {
        assertNull(ScrapedTimestamps.parse("Aug 17, 2026 · 10:00 PM"))
    }

    /** "CST" is three different offsets worldwide — a guess here is worse than no answer. */
    @Test
    fun `returns null for an ambiguous abbreviation`() {
        assertNull(ScrapedTimestamps.parse("17 Aug 2026, 22:00 CST"))
    }

    @Test
    fun `returns null rather than throwing on junk`() {
        assertNull(ScrapedTimestamps.parse(null))
        assertNull(ScrapedTimestamps.parse(""))
        assertNull(ScrapedTimestamps.parse("   "))
        assertNull(ScrapedTimestamps.parse("sometime last Tuesday"))
        assertNull(ScrapedTimestamps.parse("17 Aug 2026"))
    }
}
