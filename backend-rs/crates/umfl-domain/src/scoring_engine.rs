//! Prices a hero's involvement in a match against a set of coefficients.
//!
//! Points are computed at read time and **never stored** -- coefficients are
//! mutable reference data retuned with a bare `UPDATE`, so a stored total would
//! be a cache with nothing to invalidate it. See AGENTS.md, "Nothing writes
//! points".

use crate::match_metrics;
use crate::match_result::MetricContext;
use crate::rounding::round2;
use indexmap::IndexMap;
use rust_decimal::{Decimal, prelude::ToPrimitive};

/// One tournament's active scoring configuration, as read out of
/// `scoring_rule_sets` / `scoring_coefficients`.
///
/// `coefficients` is insertion-ordered by `sort_order`, which is what fixes the
/// leaderboard's left-to-right column order -- the backend cannot know it any
/// other way, which is also why this must stay an [`IndexMap`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoringRules {
    pub rule_set_id: i64,
    pub name: String,
    pub coefficients: IndexMap<String, Decimal>,

    /// Metric keys are matched case- and whitespace-insensitively, so `' win '`
    /// and `'Win'` cannot become separate columns. (The schema's CHECK already
    /// guards against that; this makes the domain object safe on its own.)
    by_metric: IndexMap<String, Decimal>,

    /// The metrics that actually score: known to [`match_metrics`], in column
    /// order.
    scored_metrics: Vec<String>,

    /// Configured metrics nothing implements. Worth logging once; never raised.
    unknown_metrics: Vec<String>,
}

impl ScoringRules {
    pub fn new(
        rule_set_id: i64,
        name: impl Into<String>,
        coefficients: IndexMap<String, Decimal>,
    ) -> Self {
        // A later key that normalises onto an earlier one overwrites the
        // value and keeps the earlier position; `IndexMap::insert` has
        // exactly that behaviour.
        let mut by_metric: IndexMap<String, Decimal> = IndexMap::with_capacity(coefficients.len());
        for (metric, coefficient) in &coefficients {
            by_metric.insert(match_metrics::normalise(metric), *coefficient);
        }

        let scored_metrics = by_metric
            .keys()
            .filter(|metric| match_metrics::get(metric).is_some())
            .cloned()
            .collect();

        let unknown_metrics = match_metrics::unknown(coefficients.keys().map(String::as_str));

        Self {
            rule_set_id,
            name: name.into(),
            coefficients,
            by_metric,
            scored_metrics,
            unknown_metrics,
        }
    }

    /// A tournament with no active rule set: everything scores zero.
    ///
    /// Not a `const`, because an empty `IndexMap` is not constructible in one.
    pub fn none() -> Self {
        Self::new(0, "", IndexMap::new())
    }

    pub fn scored_metrics(&self) -> &[String] {
        &self.scored_metrics
    }

    pub fn unknown_metrics(&self) -> &[String] {
        &self.unknown_metrics
    }

    pub fn coefficient_of(&self, metric: &str) -> Decimal {
        self.by_metric
            .get(&match_metrics::normalise(metric))
            .copied()
            .unwrap_or(Decimal::ZERO)
    }
}

/// Each metric's contribution, keyed by metric, in column order.
///
/// Zero contributions are omitted: a metric the hero did not earn (or one
/// weighted at zero) is absent rather than present as `0.0`. Each value is
/// rounded to 2dp **before** it is summed, so a displayed total is exactly the
/// sum of its displayed parts -- folding in `Decimal` end to end would produce
/// *better* numbers and *different* ones.
pub fn breakdown(context: &MetricContext<'_>, rules: &ScoringRules) -> IndexMap<String, f64> {
    let mut result = IndexMap::new();
    if rules.scored_metrics.is_empty() {
        return result;
    }
    for metric in &rules.scored_metrics {
        let Some(extractor) = match_metrics::get(metric) else {
            continue;
        };
        let measured = extractor(context);
        if measured == 0.0 {
            continue;
        }
        let coefficient = rules
            .coefficient_of(metric)
            .to_f64()
            .expect("a numeric(10,4) coefficient always fits an f64");
        let points = round2(measured * coefficient);
        if points != 0.0 {
            result.insert(metric.clone(), points);
        }
    }
    result
}

/// This hero's net score for this match -- may be negative.
pub fn score(context: &MetricContext<'_>, rules: &ScoringRules) -> f64 {
    round2(breakdown(context, rules).values().sum())
}

/// The fold from measured metrics to points, tested directly.
///
/// The coefficients used here are the seeded "Season 2026 Standard" weights,
/// including the deliberately unimplemented `CROWD_FAVOURITE` -- proving in a
/// plain unit test what the standings integration test then proves against the
/// real database.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_result::{
        BanResult, BanType, DraftedHeroResult, GameParticipantResult, GameResult, HeroRole,
        MatchParticipantResult, MatchResult,
    };
    use std::str::FromStr;

    fn rules(weights: &[(&str, &str)]) -> ScoringRules {
        let coefficients = weights
            .iter()
            .map(|(metric, weight)| ((*metric).to_string(), Decimal::from_str(weight).unwrap()))
            .collect();
        ScoringRules::new(1, "Season 2026 Standard", coefficients)
    }

    fn standard() -> ScoringRules {
        rules(&[
            ("WIN", "10.0000"),
            ("HEALTH_REMAINING", "0.7500"),
            ("HEALTH_DIFFERENTIAL", "0.5000"),
            ("SHUTOUT", "3.0000"),
            ("SELF_BAN", "2.0000"),
            ("OPPONENT_BAN", "2.0000"),
            ("APPEARANCE", "1.0000"),
            ("CROWD_FAVOURITE", "5.0000"),
        ])
    }

    /// Match 6 of the seed: Bigfoot 11 shuts out Beowulf 0, Sun Wukong
    /// opponent-banned.
    fn match_fixture() -> MatchResult {
        MatchResult {
            match_id: 6,
            tournament_id: 1,
            round: 2,
            played_at: "2026-06-06T11:00:00Z".parse().unwrap(),
            external_link: "https://example.com/match/scoring".into(),
            participants: vec![
                MatchParticipantResult {
                    side: 0,
                    player_label: Some("Aurelie Blanc".into()),
                    drafted_heroes: vec![DraftedHeroResult {
                        hero_id: 7,
                        hero_name: "Bigfoot".into(),
                    }],
                },
                MatchParticipantResult {
                    side: 1,
                    player_label: Some("Miles Ashworth".into()),
                    drafted_heroes: vec![DraftedHeroResult {
                        hero_id: 11,
                        hero_name: "Beowulf".into(),
                    }],
                },
            ],
            games: vec![GameResult {
                game_id: 1,
                game_number: 1,
                map_id: 3,
                map_name: "Raptor Paddock".into(),
                participants: vec![
                    GameParticipantResult {
                        side: 0,
                        hero_id: 7,
                        hero_name: "Bigfoot".into(),
                        health_remaining: 11,
                        is_winner: true,
                    },
                    GameParticipantResult {
                        side: 1,
                        hero_id: 11,
                        hero_name: "Beowulf".into(),
                        health_remaining: 0,
                        is_winner: false,
                    },
                ],
            }],
            bans: vec![BanResult {
                hero_id: 8,
                hero_name: "Sun Wukong".into(),
                ban_type: BanType::OpponentBan,
                side: None,
            }],
        }
    }

    /// The hero's per-game context -- where every metric but `APPEARANCE` is
    /// priced.
    fn context_for(m: &MatchResult, hero_id: i64) -> MetricContext<'_> {
        let mut found: Vec<MetricContext<'_>> = m
            .hero_contexts()
            .into_iter()
            .filter(|c| c.hero_id == hero_id && !matches!(c.role, HeroRole::Drafted))
            .collect();
        assert_eq!(found.len(), 1, "hero {hero_id} has no unique game context");
        found.remove(0)
    }

    /// The hero's once-per-series draft context -- where `APPEARANCE` is priced.
    fn draft_context_for(m: &MatchResult, hero_id: i64) -> MetricContext<'_> {
        let mut found: Vec<MetricContext<'_>> = m
            .hero_contexts()
            .into_iter()
            .filter(|c| c.hero_id == hero_id && matches!(c.role, HeroRole::Drafted))
            .collect();
        assert_eq!(found.len(), 1, "hero {hero_id} has no draft context");
        found.remove(0)
    }

    /// Checks contents *and* order: the column order is what the leaderboard
    /// renders left to right, so a bare map equality would not pin it.
    fn assert_breakdown(actual: &IndexMap<String, f64>, expected: &[(&str, f64)]) {
        let rendered: Vec<(&str, f64)> = actual.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(rendered, expected);
    }

    #[test]
    fn a_winning_hero_banks_every_metric_its_game_earned() {
        let m = match_fixture();
        let rules = standard();
        let bigfoot = context_for(&m, 7);

        assert_breakdown(
            &breakdown(&bigfoot, &rules),
            &[
                ("WIN", 10.0),
                ("HEALTH_REMAINING", 8.25),   // 11 * 0.75
                ("HEALTH_DIFFERENTIAL", 5.5), // (11 - 0) * 0.5
                ("SHUTOUT", 3.0),
            ],
        );
        assert_eq!(score(&bigfoot, &rules), 26.75);
    }

    #[test]
    fn the_appearance_point_rides_on_the_draft_so_the_match_total_is_game_plus_draft() {
        let m = match_fixture();
        let rules = standard();

        assert_breakdown(
            &breakdown(&draft_context_for(&m, 7), &rules),
            &[("APPEARANCE", 1.0)],
        );

        let match_total: f64 = m
            .hero_contexts()
            .iter()
            .filter(|c| c.hero_id == 7)
            .map(|c| score(c, &rules))
            .sum();
        assert_eq!(
            match_total, 27.75,
            "26.75 from the game it won, 1.00 for having been drafted"
        );
    }

    #[test]
    fn the_total_is_exactly_the_sum_of_the_breakdown() {
        let m = match_fixture();
        let rules = standard();

        for context in m.hero_contexts() {
            let parts = breakdown(&context, &rules);
            assert_eq!(
                round2(parts.values().sum()),
                score(&context, &rules),
                "hero {} as {:?}",
                context.hero_id,
                context.role
            );
        }
    }

    #[test]
    fn an_unknown_metric_contributes_nothing_and_does_not_panic() {
        let m = match_fixture();
        let rules = standard();

        assert!(!breakdown(&context_for(&m, 7), &rules).contains_key("CROWD_FAVOURITE"));
        assert_eq!(rules.unknown_metrics(), ["CROWD_FAVOURITE".to_string()]);
        assert!(
            !rules
                .scored_metrics()
                .contains(&"CROWD_FAVOURITE".to_string())
        );
    }

    #[test]
    fn scored_metrics_keep_their_configured_column_order() {
        assert_eq!(
            standard().scored_metrics(),
            [
                "WIN",
                "HEALTH_REMAINING",
                "HEALTH_DIFFERENTIAL",
                "SHUTOUT",
                "SELF_BAN",
                "OPPONENT_BAN",
                "APPEARANCE",
            ]
            .map(String::from)
        );
    }

    #[test]
    fn a_zero_coefficient_omits_the_entry_rather_than_showing_a_zero() {
        let m = match_fixture();
        let no_appearance_bonus = rules(&[("WIN", "10.0000"), ("APPEARANCE", "0.0000")]);

        assert_breakdown(
            &breakdown(&context_for(&m, 7), &no_appearance_bonus),
            &[("WIN", 10.0)],
        );
        assert!(breakdown(&draft_context_for(&m, 7), &no_appearance_bonus).is_empty());
        assert!(
            no_appearance_bonus
                .scored_metrics()
                .contains(&"APPEARANCE".to_string()),
            "it is still a column, just a worthless one"
        );
    }

    #[test]
    fn a_metric_the_hero_did_not_earn_is_absent_not_zero() {
        let m = match_fixture();
        let rules = standard();
        let beowulf = breakdown(&context_for(&m, 11), &rules);

        assert!(
            beowulf.is_empty(),
            "a shut-out loser earns nothing the standard rules price, got {beowulf:?}"
        );
        assert!(!beowulf.contains_key("WIN"));
        assert!(!beowulf.contains_key("SHUTOUT"));
        assert!(
            !beowulf.contains_key("HEALTH_REMAINING"),
            "zero health times any weight is still zero"
        );
        assert!(
            !beowulf.contains_key("HEALTH_DIFFERENTIAL"),
            "the losing side has no differential to price"
        );
        assert_breakdown(
            &breakdown(&draft_context_for(&m, 11), &rules),
            &[("APPEARANCE", 1.0)],
        );
    }

    #[test]
    fn a_banned_hero_scores_only_its_ban() {
        let m = match_fixture();
        let rules = standard();
        let sun_wukong = context_for(&m, 8);

        assert_breakdown(&breakdown(&sun_wukong, &rules), &[("OPPONENT_BAN", 2.0)]);
        assert_eq!(score(&sun_wukong, &rules), 2.0);
    }

    #[test]
    fn negative_coefficients_are_legitimate_penalties() {
        let m = match_fixture();
        let penalised = rules(&[("HEALTH_DIFFERENTIAL", "0.5000"), ("LOSS", "-6.0000")]);
        let beowulf = context_for(&m, 11);

        assert_breakdown(&breakdown(&beowulf, &penalised), &[("LOSS", -6.0)]);
        assert_eq!(score(&beowulf, &penalised), -6.0);
    }

    #[test]
    fn the_two_way_differential_carries_a_negative_contribution_to_the_total() {
        let m = match_fixture();
        // The same weight an admin would give the win-gated key, on the ungated
        // one: the shut-out loser is charged exactly what the winner is paid.
        let two_way = rules(&[("HEALTH_DIFFERENTIAL_TWO_WAY", "0.5000")]);

        let bigfoot = context_for(&m, 7);
        assert_breakdown(
            &breakdown(&bigfoot, &two_way),
            &[("HEALTH_DIFFERENTIAL_TWO_WAY", 5.5)],
        );
        assert_eq!(score(&bigfoot, &two_way), 5.5);

        let beowulf = context_for(&m, 11);
        assert_breakdown(
            &breakdown(&beowulf, &two_way),
            &[("HEALTH_DIFFERENTIAL_TWO_WAY", -5.5)],
        );
        assert_eq!(score(&beowulf, &two_way), -5.5);

        assert!(
            two_way.unknown_metrics().is_empty(),
            "the registry implements it, so it is a column and not a warning"
        );
    }

    #[test]
    fn the_two_differential_keys_are_independent_columns_priced_separately() {
        let m = match_fixture();
        let both = rules(&[
            ("HEALTH_DIFFERENTIAL", "0.5000"),
            ("HEALTH_DIFFERENTIAL_TWO_WAY", "0.5000"),
        ]);

        assert_eq!(
            both.scored_metrics(),
            ["HEALTH_DIFFERENTIAL", "HEALTH_DIFFERENTIAL_TWO_WAY"].map(String::from)
        );
        assert_breakdown(
            &breakdown(&context_for(&m, 7), &both),
            &[
                ("HEALTH_DIFFERENTIAL", 5.5),
                ("HEALTH_DIFFERENTIAL_TWO_WAY", 5.5),
            ],
        );
        assert_breakdown(
            &breakdown(&context_for(&m, 11), &both),
            &[("HEALTH_DIFFERENTIAL_TWO_WAY", -5.5)],
        );
    }

    #[test]
    fn metric_keys_are_matched_case_and_whitespace_insensitively() {
        let m = match_fixture();
        let sloppy = rules(&[(" win ", "10.0000"), ("Appearance", "1.0000")]);

        assert_eq!(
            sloppy.scored_metrics(),
            ["WIN", "APPEARANCE"].map(String::from)
        );
        assert_breakdown(&breakdown(&context_for(&m, 7), &sloppy), &[("WIN", 10.0)]);
        assert_breakdown(
            &breakdown(&draft_context_for(&m, 7), &sloppy),
            &[("APPEARANCE", 1.0)],
        );
        assert!(sloppy.unknown_metrics().is_empty());
    }

    #[test]
    fn each_contribution_is_rounded_before_it_is_summed() {
        let m = match_fixture();
        // 11 health at 0.333 is 3.663, which must round to 3.66 before joining
        // the total.
        let awkward = rules(&[("HEALTH_REMAINING", "0.3330")]);
        let bigfoot = context_for(&m, 7);

        assert_breakdown(
            &breakdown(&bigfoot, &awkward),
            &[("HEALTH_REMAINING", 3.66)],
        );
        assert_eq!(score(&bigfoot, &awkward), 3.66);
    }

    #[test]
    fn a_tournament_with_no_active_rule_set_scores_nothing() {
        let m = match_fixture();
        let none = ScoringRules::none();
        let bigfoot = context_for(&m, 7);

        assert!(breakdown(&bigfoot, &none).is_empty());
        assert_eq!(score(&bigfoot, &none), 0.0);
        assert_eq!(none.name, "");
        assert!(none.scored_metrics().is_empty());
    }
}
