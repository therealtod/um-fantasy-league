package com.umfl.matchimport

import java.time.Instant
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/**
 * Parses the source site's rendered timestamp, e.g. `"17 Aug 2026, 22:00 CEST"`.
 *
 * Pure and best-effort by design: **it returns null rather than throwing.** The
 * timestamp is rendered text in whatever timezone the source happened to display,
 * abbreviated in a way that is genuinely ambiguous worldwide ("CST" alone is three
 * different offsets). Letting that ambiguity fail an import would trade a whole
 * scraped match — drafts, bans, per-game health — for one field the admin can set
 * in two clicks in a date picker that already defaults to now.
 *
 * So: parse what is unambiguous, hand back null for the rest, and let the caller
 * surface the raw string alongside it.
 */
object ScrapedTimestamps {

    /**
     * European abbreviations only — the competition this was built against runs
     * on CEST, and a guess at a globally ambiguous abbreviation is worse than
     * no answer. Anything absent here yields null and becomes the admin's call.
     */
    private val KNOWN_ZONES = mapOf(
        "UTC" to "UTC",
        "GMT" to "UTC",
        "BST" to "Europe/London",
        "CET" to "Europe/Paris",
        "CEST" to "Europe/Paris",
        "EET" to "Europe/Helsinki",
        "EEST" to "Europe/Helsinki",
        "WET" to "Europe/Lisbon",
        "WEST" to "Europe/Lisbon",
    )

    private val PATTERNS = listOf(
        // The detail page's shape: "17 Aug 2026, 22:00"
        DateTimeFormatter.ofPattern("d MMM uuuu, HH:mm", Locale.ENGLISH),
        // The list page's shape, should it ever reach here: "Aug 17, 2026 · 10:00 PM"
        DateTimeFormatter.ofPattern("MMM d, uuuu · hh:mm a", Locale.ENGLISH),
    )

    /** Splits a trailing alphabetic timezone abbreviation off the datetime text. */
    private val TRAILING_ZONE = Regex("""^(.*?)[\s]+([A-Z]{2,5})$""")

    fun parse(raw: String?): Instant? {
        val text = raw?.trim()?.takeIf { it.isNotEmpty() } ?: return null

        val match = TRAILING_ZONE.find(text)
        val (dateTimeText, zone) = if (match != null) {
            val abbreviation = match.groupValues[2]
            // A trailing "AM"/"PM" is part of the time, not a zone.
            if (abbreviation in setOf("AM", "PM")) {
                text to null
            } else {
                match.groupValues[1] to (KNOWN_ZONES[abbreviation] ?: return null)
            }
        } else {
            text to null
        }

        // Without a zone there is no instant to compute — a local time is not a
        // point on the timeline. Better to leave it to the admin than to invent
        // a timezone and file the match at the wrong hour.
        val zoneId = zone?.let { ZoneId.of(it) } ?: return null

        for (pattern in PATTERNS) {
            val parsed = runCatching { LocalDateTime.parse(dateTimeText.trim(), pattern) }.getOrNull()
            if (parsed != null) return parsed.atZone(zoneId).toInstant()
        }
        return null
    }
}
