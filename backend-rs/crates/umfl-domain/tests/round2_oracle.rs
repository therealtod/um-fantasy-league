//! The differential gate for [`umfl_domain::rounding::round2`].
//!
//! `round2` is `BigDecimal.valueOf(d).setScale(2, HALF_UP).toDouble()`, and
//! every number the leaderboard prints has been through it. Reasoning about
//! whether a Rust reimplementation agrees with the JDK is not good enough: the
//! disagreements live in the third decimal of values like `2.675`, where the
//! shortest round-tripping decimal string and the exact binary expansion of the
//! same `f64` fall on opposite sides of the tie.
//!
//! So the expectations are *printed by the JDK*, not written by hand.
//! `Round2Oracle.java` (kept out of the repo -- it is a throwaway, and its full
//! output is ~154k rows) emits `input,output` with both columns as
//! `Double.toString`, over uniform randoms across nine magnitudes, values with
//! 3..17 decimal places, exact `n.nn5` midpoints of both signs, the two `f64`
//! neighbours of each midpoint, and sums of two already-rounded metrics -- the
//! shape `ScoringEngine.score` actually produces.
//!
//! `round2_cases.csv` beside this file is a representative ~420-row slice of
//! that run: every hand-picked case, a spread of midpoints, and a stride
//! through the rest. Point `UMFL_ROUND2_ORACLE_CSV` at a full run to replay the
//! whole sample.
//!
//! Both columns parse as `f64` rather than compare as strings on purpose:
//! Java's `Double.toString` switches to `1.0E-5` below 1e-3 and above 1e7,
//! where Rust's `Display` never does. The *digits* agree -- both are
//! shortest-round-trip on JDK 19+ -- so the values are identical and only the
//! rendering differs.

use umfl_domain::rounding::round2;

fn oracle_csv() -> String {
    match std::env::var("UMFL_ROUND2_ORACLE_CSV") {
        Ok(path) => std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("UMFL_ROUND2_ORACLE_CSV={path}: {e}")),
        Err(_) => include_str!("round2_cases.csv").to_string(),
    }
}

/// `input`, `expected`, and the 1-based line number for the failure message.
fn rows(csv: &str) -> Vec<(f64, f64, usize)> {
    csv.lines()
        .enumerate()
        .skip(1) // header
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let (raw_in, raw_out) = line
                .split_once(',')
                .unwrap_or_else(|| panic!("line {}: not a pair: {line}", index + 1));
            (
                raw_in.parse().expect("an input the JDK printed"),
                raw_out.parse().expect("an output the JDK printed"),
                index + 1,
            )
        })
        .collect()
}

#[test]
fn reproduces_the_jdk_on_every_row() {
    let csv = oracle_csv();
    let rows = rows(&csv);
    assert!(rows.len() >= 400, "fixture shrank to {} rows", rows.len());

    let mut mismatches = Vec::new();
    for (input, expected, line) in &rows {
        let actual = round2(*input);
        // Bit equality, not `==`: it separates +0.0 from -0.0, and BigDecimal's
        // lack of a signed zero is exactly the kind of detail this gate exists
        // to catch. Every other value here is an exact 2dp double, so bit
        // equality and numeric equality coincide.
        if actual.to_bits() != expected.to_bits() {
            mismatches.push(format!(
                "line {line}: round2({input}) = {actual}, JDK says {expected}"
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} rows disagree with the JDK:\n{}",
        mismatches.len(),
        rows.len(),
        mismatches.join("\n"),
    );
}

/// The rows that decide the rounding *mode*, asserted by name so a fixture
/// regenerated smaller cannot quietly drop them.
#[test]
fn the_fixture_still_carries_the_cases_that_decide_the_mode() {
    let csv = include_str!("round2_cases.csv");
    for case in [
        "2.675,2.68",
        "0.125,0.13",
        "1.115,1.12",
        // HALF_UP goes away from zero: -1.01, not -1.00.
        "-1.005,-1.01",
        "0.0,0.0",
        // BigDecimal has no negative zero, so this is +0.0 on both sides.
        "-0.0,0.0",
    ] {
        assert!(
            csv.lines().any(|line| line == case),
            "fixture lost `{case}`",
        );
    }
}

/// A midpoint at the third decimal is the only input class where HALF_UP and
/// HALF_EVEN can disagree, so the slice has to keep a body of them.
#[test]
fn the_fixture_keeps_a_body_of_third_decimal_midpoints() {
    let csv = include_str!("round2_cases.csv");
    let midpoints = csv
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(','))
        .filter(|(input, _)| {
            input
                .split_once('.')
                .is_some_and(|(_, frac)| frac.len() == 3 && frac.ends_with('5'))
        })
        .count();
    assert!(
        midpoints >= 100,
        "only {midpoints} midpoints in the fixture"
    );
}
