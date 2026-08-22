//! `BigDecimal` on the wire, with its scale intact.
//!
//! Oracle: Jackson's `NumberSerializers`/`BigDecimalDeserializer`, as
//! `ScoringCoefficientDto.coefficient` exercises them.
//!
//! Three things have to line up at once, and no off-the-shelf combination does
//! all three:
//!
//! * **It is a JSON *number*, not a string.** `rust_decimal`'s own `Serialize`
//!   emits `"12.0000"` by default; Jackson emits `12.0000`, and
//!   `types.ts` declares `coefficient: number`.
//! * **Trailing zeros survive.** `numeric(10,4)` decodes as scale 4, and
//!   `BigDecimal.toString()` prints every one of those digits. Going through
//!   `f64` would print `12.0`, which is a different byte sequence for the same
//!   value.
//! * **The scale the client *sent* survives too.** `AdminScoringService.create`
//!   echoes back the aggregate it saved, whose coefficients are the submitted
//!   `BigDecimal`s rather than anything re-read from the database — so a posted
//!   `12.0` comes back `12.0`, not `12` and not `12.0000`. Parsing through
//!   `f64` loses that as well.
//!
//! Both directions therefore go through `serde_json::value::RawValue`, which is
//! the one way to hand `serde_json` a number token it did not build from a
//! primitive. That ties this module to `serde_json` as the format — which every
//! response in this crate already is.

use std::str::FromStr;

use rust_decimal::Decimal;
use serde::de::{Deserialize, Deserializer, Error as _};
use serde::ser::{Error as _, Serialize, Serializer};
use serde_json::value::RawValue;

pub fn serialize<S: Serializer>(value: &Decimal, serializer: S) -> Result<S::Ok, S::Error> {
    // `Decimal::to_string` is `BigDecimal.toString()` for every value this
    // schema can hold: plain digits, no exponent, scale preserved.
    let raw = RawValue::from_string(value.to_string()).map_err(S::Error::custom)?;
    raw.serialize(serializer)
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
    let raw = <&RawValue>::deserialize(deserializer)?;
    parse(raw.get()).ok_or_else(|| {
        D::Error::custom(format!(
            "cannot deserialize value as decimal: {}",
            raw.get()
        ))
    })
}

/// The literal JSON token, as a decimal.
///
/// A quoted token is accepted because Jackson's `BigDecimal` deserializer
/// accepts one: `{"coefficient": "1.5"}` is a legal body against the Kotlin and
/// has to stay one here. Nothing in the frontend sends that shape, but a
/// rejection would be a wire change all the same.
fn parse(token: &str) -> Option<Decimal> {
    let unquoted = token
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(token);
    Decimal::from_str(unquoted).ok()
}

/// The same, for an `Option` field — a request DTO whose `@NotNull` is enforced
/// by `garde` rather than by the type, so the absent case has to survive
/// deserialization to be reported as a validation failure.
pub mod option {
    use super::{Decimal, Deserialize, Deserializer, RawValue};

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Decimal>, D::Error> {
        // `Option<&RawValue>` rather than `#[serde(default)]` plus the scalar
        // form: an explicit `null` has to land as `None` (the `@NotNull`
        // message), not as a deserialization error (`Failed to read request`).
        let Some(raw) = Option::<&RawValue>::deserialize(deserializer)? else {
            return Ok(None);
        };
        if raw.get() == "null" {
            return Ok(None);
        }
        Ok(super::parse(raw.get()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct Holder {
        #[serde(with = "super")]
        coefficient: Decimal,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct OptHolder {
        #[serde(with = "super::option")]
        coefficient: Option<Decimal>,
    }

    #[test]
    fn a_coefficient_is_a_json_number_not_a_string() {
        let json = serde_json::to_string(&Holder {
            coefficient: Decimal::from_str("12.0000").unwrap(),
        })
        .unwrap();
        assert_eq!(json, r#"{"coefficient":12.0000}"#);
    }

    /// `numeric(10,4)` decodes at scale 4 and `BigDecimal.toString()` prints
    /// all of it; an `f64` round trip would print `0.75`.
    #[test]
    fn trailing_zeros_survive_the_round_trip() {
        let holder: Holder = serde_json::from_str(r#"{"coefficient":0.7500}"#).unwrap();
        assert_eq!(holder.coefficient.to_string(), "0.7500");
        assert_eq!(
            serde_json::to_string(&holder).unwrap(),
            r#"{"coefficient":0.7500}"#
        );
    }

    /// The scale the admin submitted is the scale `create` echoes, because the
    /// response is the saved aggregate rather than a re-read row.
    #[test]
    fn the_submitted_scale_is_the_echoed_scale() {
        for token in ["12", "12.0", "12.00"] {
            let holder: Holder =
                serde_json::from_str(&format!(r#"{{"coefficient":{token}}}"#)).unwrap();
            assert_eq!(holder.coefficient.to_string(), token);
        }
    }

    #[test]
    fn a_negative_weight_is_a_penalty_and_parses_as_one() {
        let holder: Holder = serde_json::from_str(r#"{"coefficient":-1.5}"#).unwrap();
        assert_eq!(holder.coefficient, Decimal::from_str("-1.5").unwrap());
    }

    /// Jackson's `BigDecimal` deserializer accepts a quoted number, so this
    /// one does too.
    #[test]
    fn a_quoted_number_is_accepted_as_jackson_accepts_one() {
        let holder: Holder = serde_json::from_str(r#"{"coefficient":"1.5"}"#).unwrap();
        assert_eq!(holder.coefficient, Decimal::from_str("1.5").unwrap());
    }

    #[test]
    fn a_non_numeric_token_is_a_deserialization_error() {
        assert!(serde_json::from_str::<Holder>(r#"{"coefficient":"abc"}"#).is_err());
    }

    /// An omitted or null coefficient must reach `garde` as `None` so the
    /// `@NotNull` message is what the client sees.
    #[test]
    fn an_explicit_null_is_none_rather_than_an_error() {
        let holder: OptHolder = serde_json::from_str(r#"{"coefficient":null}"#).unwrap();
        assert_eq!(holder.coefficient, None);
    }
}
