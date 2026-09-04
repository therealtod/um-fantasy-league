//! Parses the source site's rendered timestamp, e.g. `"17 Aug 2026, 22:00 CEST"`.
//!
//! Pure and best-effort by design: **it returns `None` rather than failing.**
//! The timestamp is rendered text in whatever timezone the source happened to
//! display, abbreviated in a way that is genuinely ambiguous worldwide ("CST"
//! alone is three different offsets). Letting that ambiguity fail an import
//! would trade a whole scraped match -- drafts, bans, per-game health -- for one
//! field the admin can set in two clicks in a date picker that already defaults
//! to now.
//!
//! So: parse what is unambiguous, hand back `None` for the rest, and let the
//! caller surface the raw string alongside it.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

/// European abbreviations only -- the competition this was built against runs
/// on CEST, and a guess at a globally ambiguous abbreviation is worse than no
/// answer. Anything absent here yields `None` and becomes the admin's call.
const KNOWN_ZONES: &[(&str, &str)] = &[
    ("UTC", "UTC"),
    ("GMT", "UTC"),
    ("BST", "Europe/London"),
    ("CET", "Europe/Paris"),
    ("CEST", "Europe/Paris"),
    ("EET", "Europe/Helsinki"),
    ("EEST", "Europe/Helsinki"),
    ("WET", "Europe/Lisbon"),
    ("WEST", "Europe/Lisbon"),
];

/// The detail page's shape (`"17 Aug 2026, 22:00"`) and the list page's, should
/// it ever reach here (`"Aug 17, 2026 · 10:00 PM"`).
///
/// chrono has no `d`/`MMM` pattern language, so these are `strftime` forms:
/// `%e` is a space-padded day, which `trim`ming the parsed text makes equivalent
/// to Java's `d`, and `%l` is the space-padded 12-hour clock.
const PATTERNS: &[&str] = &["%e %b %Y, %H:%M", "%b %e, %Y · %l:%M %p"];

/// A trailing `AM`/`PM` is part of the time, not a zone.
const MERIDIEMS: &[&str] = &["AM", "PM"];

pub fn parse(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let text = raw?.trim();
    if text.is_empty() {
        return None;
    }

    // Splits a trailing alphabetic zone abbreviation off the text, without
    // pulling in a regex engine for it.
    let (date_time_text, zone) = match split_trailing_zone(text) {
        Some((head, abbreviation)) if !MERIDIEMS.contains(&abbreviation) => {
            (head, Some(lookup_zone(abbreviation)?))
        }
        // No trailing zone, or one that is really a meridiem: keep the whole
        // string and fall through to the no-zone case below.
        _ => (text, None),
    };

    // Without a zone there is no instant to compute -- a local time is not a
    // point on the timeline. Better to leave it to the admin than to invent a
    // timezone and file the match at the wrong hour.
    let zone = zone?;

    for pattern in PATTERNS {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(date_time_text.trim(), pattern) {
            // `.single()` rather than `.earliest()`: a wall time inside a DST
            // gap or fold is exactly the ambiguity this module refuses to
            // guess at.
            return zone
                .from_local_datetime(&parsed)
                .single()
                .map(|zoned| zoned.with_timezone(&Utc));
        }
    }
    None
}

/// The text before a trailing whitespace-separated run of 2-5 uppercase
/// letters, and that run.
fn split_trailing_zone(text: &str) -> Option<(&str, &str)> {
    let (head, last) = text.rsplit_once(char::is_whitespace)?;
    let legal =
        (2..=5).contains(&last.chars().count()) && last.chars().all(|c| c.is_ascii_uppercase());
    legal.then_some((head, last))
}

fn lookup_zone(abbreviation: &str) -> Option<Tz> {
    KNOWN_ZONES
        .iter()
        .find(|(key, _)| *key == abbreviation)
        .map(|(_, zone)| zone.parse().expect("KNOWN_ZONES names real IANA zones"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    /// The detail page's real shape, captured from a live scrape.
    #[test]
    fn parses_the_detail_pages_timezone_qualified_timestamp() {
        // 22:00 CEST is UTC+2, so 20:00Z.
        assert_eq!(
            parse(Some("17 Aug 2026, 22:00 CEST")),
            Some(instant("2026-08-17T20:00:00Z"))
        );
    }

    #[test]
    fn parses_a_winter_cet_timestamp_at_the_other_offset() {
        assert_eq!(
            parse(Some("17 Jan 2026, 22:00 CET")),
            Some(instant("2026-01-17T21:00:00Z"))
        );
    }

    #[test]
    fn parses_utc() {
        assert_eq!(
            parse(Some("17 Aug 2026, 22:00 UTC")),
            Some(instant("2026-08-17T22:00:00Z"))
        );
    }

    /// A local time with no zone is not a point on the timeline. Returning
    /// `None` hands the field to the admin rather than filing the match at a
    /// guessed hour.
    #[test]
    fn returns_none_for_the_list_pages_zoneless_timestamp() {
        assert_eq!(parse(Some("Aug 17, 2026 · 10:00 PM")), None);
    }

    /// "CST" is three different offsets worldwide -- a guess here is worse than
    /// no answer.
    #[test]
    fn returns_none_for_an_ambiguous_abbreviation() {
        assert_eq!(parse(Some("17 Aug 2026, 22:00 CST")), None);
    }

    #[test]
    fn returns_none_rather_than_failing_on_junk() {
        assert_eq!(parse(None), None);
        assert_eq!(parse(Some("")), None);
        assert_eq!(parse(Some("   ")), None);
        assert_eq!(parse(Some("sometime last Tuesday")), None);
        assert_eq!(parse(Some("17 Aug 2026")), None);
    }

    /// The zone abbreviation carries the offset, so a single-digit day still
    /// parses -- Java's `d` is not zero-padded and neither is the source.
    #[test]
    fn parses_a_single_digit_day() {
        assert_eq!(
            parse(Some("7 Aug 2026, 09:05 UTC")),
            Some(instant("2026-08-07T09:05:00Z"))
        );
    }

    /// Every zone in the table has to name a real IANA region, since the lookup
    /// asserts on it rather than carrying a fallback.
    #[test]
    fn every_known_zone_resolves() {
        for (abbreviation, _) in KNOWN_ZONES {
            assert!(
                lookup_zone(abbreviation).is_some(),
                "{abbreviation} does not resolve"
            );
        }
    }
}
