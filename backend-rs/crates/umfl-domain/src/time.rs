//! Wire-format timestamps, byte-compatible with Java's `Instant.toString()`.
//!
//! The Kotlin backend serializes every `Instant` through Jackson with
//! `WRITE_DATES_AS_TIMESTAMPS` disabled, which calls `Instant.toString()` --
//! i.e. `DateTimeFormatter.ISO_INSTANT`. That format has one property no
//! chrono helper reproduces: **the fractional part is emitted in groups of
//! three digits, using the fewest groups that represent the value exactly** --
//! 0, 3, 6 or 9 digits -- and the zone is always the literal `Z`.
//!
//! `chrono`'s `to_rfc3339()` emits `+00:00` instead of `Z`; its
//! `to_rfc3339_opts(SecondsFormat::Millis, true)` emits a *fixed* precision and
//! so writes `.000` where Java writes nothing. Either would change the wire
//! contract on every `playedAt`, `registeredAt` and `lockedAt`.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serializer};

/// Renders a timestamp exactly as `java.time.Instant::toString` would.
pub fn format_instant(ts: &DateTime<Utc>) -> String {
    let nanos = ts.timestamp_subsec_nanos();
    // A leap second is represented by chrono as nanos >= 1_000_000_000. Postgres
    // `timestamptz` cannot produce one, so fall back rather than mis-format.
    if nanos >= 1_000_000_000 {
        return ts.to_rfc3339_opts(SecondsFormat::AutoSi, true);
    }
    let head = ts.format("%Y-%m-%dT%H:%M:%S");
    if nanos == 0 {
        format!("{head}Z")
    } else if nanos.is_multiple_of(1_000_000) {
        format!("{head}.{:03}Z", nanos / 1_000_000)
    } else if nanos.is_multiple_of(1_000) {
        format!("{head}.{:06}Z", nanos / 1_000)
    } else {
        format!("{head}.{nanos:09}Z")
    }
}

/// `#[serde(with = "umfl_domain::time::java_instant")]`
pub mod java_instant {
    use super::*;

    pub fn serialize<S: Serializer>(ts: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format_instant(ts))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let raw = String::deserialize(d)?;
        DateTime::parse_from_rfc3339(&raw)
            .map(|t| t.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

/// The same, for an `Option`. Paired with `skip_serializing_if` at the field,
/// since `default-property-inclusion: non_null` means an absent timestamp is
/// omitted from the document rather than emitted as `null`.
pub mod java_instant_opt {
    use super::*;

    pub fn serialize<S: Serializer>(ts: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
        match ts {
            Some(t) => s.serialize_str(&format_instant(t)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
        let raw = Option::<String>::deserialize(d)?;
        raw.map(|r| {
            DateTime::parse_from_rfc3339(&r)
                .map(|t| t.with_timezone(&Utc))
                .map_err(serde::de::Error::custom)
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(s: i64, nanos: u32) -> DateTime<Utc> {
        Utc.timestamp_opt(s, nanos).single().expect("valid instant")
    }

    /// Each expectation is what `Instant.toString()` prints in Java 21.
    #[test]
    fn matches_java_instant_to_string() {
        // Whole seconds: no fractional part at all -- NOT ".000".
        assert_eq!(format_instant(&at(0, 0)), "1970-01-01T00:00:00Z");
        // The seed's first match, as `V3__demo_fixtures.sql` records it.
        assert_eq!(
            format_instant(&at(1_780_684_200, 0)),
            "2026-06-05T18:30:00Z"
        );
        // Milliseconds -> exactly 3 digits.
        assert_eq!(
            format_instant(&at(0, 500_000_000)),
            "1970-01-01T00:00:00.500Z"
        );
        assert_eq!(
            format_instant(&at(0, 1_000_000)),
            "1970-01-01T00:00:00.001Z"
        );
        // Microseconds -> exactly 6. This is the precision Postgres timestamptz
        // actually stores, so it is the common non-zero case.
        assert_eq!(
            format_instant(&at(0, 123_456_000)),
            "1970-01-01T00:00:00.123456Z"
        );
        // Nanoseconds -> all 9.
        assert_eq!(format_instant(&at(0, 1)), "1970-01-01T00:00:00.000000001Z");
        assert_eq!(
            format_instant(&at(0, 123_456_789)),
            "1970-01-01T00:00:00.123456789Z"
        );
    }

    #[test]
    fn never_emits_the_offset_form_chrono_defaults_to() {
        let rendered = format_instant(&at(1_780_684_200, 0));
        assert!(rendered.ends_with('Z'), "{rendered}");
        assert!(!rendered.contains("+00:00"), "{rendered}");
    }

    #[test]
    fn round_trips_through_serde() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Probe {
            #[serde(with = "java_instant")]
            played_at: DateTime<Utc>,
        }
        let probe = Probe {
            played_at: at(1_780_684_200, 123_456_000),
        };
        let json = serde_json::to_string(&probe).expect("serialize");
        assert_eq!(json, r#"{"played_at":"2026-06-05T18:30:00.123456Z"}"#);
        assert_eq!(
            serde_json::from_str::<Probe>(&json).expect("deserialize"),
            probe
        );
    }
}
