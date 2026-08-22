//! Half-up to 2dp, the way `ScoringEngine.round2` does it.
//!
//! Every number the leaderboard prints has been through this function, and the
//! parity suite asserts the seed's board with exact `f64` equality
//! (ArthurianLegend 100.00, NeonStrategist 79.75, ...). A rounding mode that is
//! merely *close* would not show up until a coefficient was retuned.

use rust_decimal::{Decimal, RoundingStrategy, prelude::ToPrimitive};
use std::str::FromStr;

/// The Kotlin is `BigDecimal.valueOf(value).setScale(2, HALF_UP).toDouble()`.
///
/// Three things in that one line are load-bearing, and none of them survives an
/// "obvious" simplification:
///
/// * `BigDecimal.valueOf(double)` is specified as `new BigDecimal(Double
///   .toString(d))` -- the **shortest decimal string that round-trips**, not the
///   exact binary expansion. `2.675` is really `2.67499999999999982...` as an
///   `f64`, and the difference decides whether it rounds to `2.68` or `2.67`.
///   Rust's `f64` `Display` is shortest-round-trip too, so the `format!` is what
///   makes the two agree. `Decimal::from_f64_retain` takes the other reading and
///   is wrong here.
/// * `MidpointAwayFromZero` **is** `HALF_UP`. `MidpointNearestEven` is
///   `HALF_EVEN`, which would send `0.125` to `0.12` and `-1.005` to `-1.00`.
/// * Negative midpoints go *away* from zero: `-1.005` is `-1.01`, not `-1.00`.
///
/// `-0.0` comes back as `+0.0`, because `BigDecimal` has no signed zero and
/// `doubleValue()` on a zero returns the positive one. `tests/round2_oracle.rs`
/// pins that against the JDK rather than against reasoning.
///
/// # Panics
///
/// On a value `Decimal` cannot represent: NaN, an infinity, or a magnitude past
/// ~7.9e28. `BigDecimal.valueOf` throws `NumberFormatException` on the first
/// two, so both runtimes answer 500 there; the third is unreachable from a
/// `numeric(10,4)` coefficient times a health total, and a silent fall-through
/// to an unrounded value would be far worse than a stack trace naming it.
pub fn round2(value: f64) -> f64 {
    Decimal::from_str(&format!("{value}"))
        .unwrap_or_else(|e| panic!("round2: {value} is not representable as a decimal: {e}"))
        .round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
        .to_f64()
        .expect("a 2dp decimal always fits an f64")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cases that separate HALF_UP from HALF_EVEN, and
    /// `BigDecimal.valueOf` from `new BigDecimal(double)`. Every expectation
    /// here was printed by the JDK; see `tests/round2_oracle.rs` for the rest.
    #[test]
    fn matches_the_jdk_on_the_cases_that_decide_the_mode() {
        // Shortest-round-trip, not the binary expansion: 2.675 as an f64 is
        // just under the midpoint, so an exact-binary reading gives 2.67.
        assert_eq!(round2(2.675), 2.68);
        assert_eq!(round2(0.125), 0.13); // HALF_EVEN would say 0.12
        assert_eq!(round2(1.115), 1.12);
        // Away from zero, not toward it, and not toward even.
        assert_eq!(round2(-1.005), -1.01);
        assert_eq!(round2(-2.675), -2.68);
        assert_eq!(round2(-0.125), -0.13);
    }

    #[test]
    fn zero_comes_back_unsigned_as_bigdecimal_does() {
        assert_eq!(round2(0.0), 0.0);
        assert_eq!(round2(-0.0), 0.0);
        // BigDecimal has no negative zero, so neither does this.
        assert!(round2(-0.0).is_sign_positive());
        // ... including when a negative input rounds down to nothing.
        assert!(round2(-0.001).is_sign_positive());
    }

    /// `ScoringEngine.score` rounds each metric and then rounds their sum, so
    /// round2 is idempotent on its own output by construction.
    #[test]
    fn is_idempotent() {
        for raw in [2.675, -1.005, 0.1 + 0.2, 79.745, -0.004] {
            let once = round2(raw);
            assert_eq!(round2(once), once, "{raw}");
        }
    }
}
