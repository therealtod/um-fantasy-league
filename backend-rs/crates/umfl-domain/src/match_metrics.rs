//! The registry of scoring metrics this application knows how to measure.
//!
//! A direct port of `scoring/MatchMetrics.kt`, minus the `MetricContext`/
//! `HeroRole` pair it declares alongside the registry -- those live in
//! [`crate::match_result`], because `MatchResult::hero_contexts` is their only
//! constructor and splitting them would make the two modules circular.
//!
//! `scoring_coefficients.metric` is free-form text so an admin can add a weighted
//! row without a migration. This registry prices the keys it implements and
//! **silently ignores the rest**: an unknown metric contributes nothing, is
//! dropped from the leaderboard's columns, and raises nothing. The seed's
//! `CROWD_FAVOURITE` is the deliberate proof of that; leave it unimplemented.
//!
//! There is deliberately no `DAMAGE_DEALT`: `heroes` carries no starting-health
//! column, so damage is not derivable from `health_remaining`. Adding it is a
//! schema decision, not a registry one.

use crate::match_result::{BanType, HeroRole, MetricContext};

/// What an extractor is: a pure measurement of one hero's role in one match.
///
/// The lifetime is elided and therefore higher-ranked, so one function pointer
/// serves contexts borrowed from any match.
pub type Extractor = fn(&MetricContext<'_>) -> f64;

/// The registry, in declaration order.
///
/// A slice rather than a map: there are nine entries, a linear scan is free at
/// this size, and a `const` array cannot drift out of the order the leaderboard
/// reads it in the way a lazily built `HashMap` could. Kotlin uses
/// `linkedMapOf` for exactly the same ordering reason.
const EXTRACTORS: &[(&str, Extractor)] = &[
    ("APPEARANCE", appearance),
    ("SELF_BAN", self_ban),
    ("OPPONENT_BAN", opponent_ban),
    ("WIN", win),
    ("LOSS", loss),
    ("HEALTH_REMAINING", health_remaining),
    ("HEALTH_DIFFERENTIAL", health_differential),
    ("HEALTH_DIFFERENTIAL_TWO_WAY", health_differential_two_way),
    ("SHUTOUT", shutout),
];

/// Every metric key this build implements, in a stable order.
pub fn known() -> impl Iterator<Item = &'static str> {
    EXTRACTORS.iter().map(|(name, _)| *name)
}

/// The extractor for `metric`, or `None` when nothing implements it.
///
/// Kotlin spells this `MatchMetrics[metric]`.
pub fn get(metric: &str) -> Option<Extractor> {
    let normalised = normalise(metric);
    EXTRACTORS
        .iter()
        .find(|(name, _)| *name == normalised)
        .map(|(_, extractor)| *extractor)
}

/// Trim and upper-case, so `' win '` and `'Win'` are the same column.
pub fn normalise(metric: &str) -> String {
    metric.trim().to_uppercase()
}

/// The subset of `metrics` no extractor implements -- typically a typo.
///
/// Normalised, de-duplicated, and returned in first-encounter order, matching
/// Kotlin's `map(::normalise).distinct().filterNot(...)`.
pub fn unknown<'a>(metrics: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for metric in metrics {
        let normalised = normalise(metric);
        if !seen.contains(&normalised) {
            seen.push(normalised);
        }
    }
    seen.retain(|metric| !EXTRACTORS.iter().any(|(name, _)| name == metric));
    seen
}

/// `HEALTH_REMAINING` -> `Health Remaining`, for the leaderboard header.
pub fn label(metric: &str) -> String {
    normalise(metric)
        .split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let lowered = word.to_lowercase();
            let mut chars = lowered.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// The extractors below are free functions for the same reason the Kotlin's are
// named rather than inline lambdas: several need an early return.

/// A hero featured in this match's draft and not banned out of it -- whether or
/// not it went on to play a game. Scored off the draft rather than off a
/// participant row precisely so a hero taken and never fielded still counts,
/// and scored once for the series rather than once per game: a side drafts
/// before game 1, not again before game 2.
fn appearance(context: &MetricContext<'_>) -> f64 {
    if matches!(context.role, HeroRole::Drafted) {
        1.0
    } else {
        0.0
    }
}

/// A hero its own side banned out. `HeroRole::Banned` stays a bare marker; only
/// these two extractors look past it into the match's bans to see which
/// category applied.
fn self_ban(context: &MetricContext<'_>) -> f64 {
    ban_of_type(context, BanType::SelfBan)
}

/// A hero the opposing side banned out. `PRE_BAN` -- struck before sides are
/// known -- is priced by neither this nor [`self_ban`]: nobody chose to deny it
/// to a particular opponent.
fn opponent_ban(context: &MetricContext<'_>) -> f64 {
    ban_of_type(context, BanType::OpponentBan)
}

/// Prices a ban by category alone. It deliberately never reads `hero_bans.side`:
/// points are per hero and never per player (AGENTS.md, "A ban's `side` is the
/// draft it came out of, not who struck it").
fn ban_of_type(context: &MetricContext<'_>, ban_type: BanType) -> f64 {
    if !matches!(context.role, HeroRole::Banned) {
        return 0.0;
    }
    let found = context
        .match_result
        .bans
        .iter()
        .find(|ban| ban.hero_id == context.hero_id)
        .map(|ban| ban.ban_type);
    if found == Some(ban_type) { 1.0 } else { 0.0 }
}

fn win(context: &MetricContext<'_>) -> f64 {
    match context.participant() {
        Some(participant) if participant.is_winner => 1.0,
        _ => 0.0,
    }
}

/// The other side of [`win`], and exhaustive with it: every game has exactly one
/// winner (`MatchRule::NOT_EXACTLY_ONE_WINNER`), so within a game a hero that
/// did not win, lost. There is deliberately no `DRAW` extractor to pair with
/// these -- a drawn game is not a recordable result, so pricing one would be
/// pricing something that cannot happen, and the registry surfaces `DRAW` as an
/// unknown-metric warning instead.
fn loss(context: &MetricContext<'_>) -> f64 {
    match context.participant() {
        Some(participant) => {
            if participant.is_winner {
                0.0
            } else {
                1.0
            }
        }
        None => 0.0,
    }
}

fn health_remaining(context: &MetricContext<'_>) -> f64 {
    context
        .participant()
        .map_or(0.0, |p| f64::from(p.health_remaining))
}

/// This hero's health minus the healthiest opponent's, in one game it played.
/// `None` when the hero did not play the game or had no opponent in it -- both
/// differential metrics read as 0.0 in that case, they only differ on the
/// losing side.
///
/// Measured against the *healthiest* opponent rather than "the loser" so it
/// generalises past two sides: every hero is priced against the best of the
/// rest, whichever of them won.
fn health_gap(context: &MetricContext<'_>) -> Option<f64> {
    let participant = context.participant()?;
    let best = context
        .opponents()
        .iter()
        .map(|o| o.health_remaining)
        .max()?;
    Some(f64::from(participant.health_remaining - best))
}

/// The [`health_gap`] of a game this hero **won**. Unlike [`win`]/[`loss`] this
/// is not symmetric across the two sides -- a losing hero scores 0.0 here
/// regardless of how much health it had left, since there is no losing side of
/// this metric to price.
///
/// [`health_differential_two_way`] is the ungated variant of exactly this. The
/// two are a deliberate pair rather than a duplication: an admin prices one or
/// the other (or both) by adding the matching `scoring_coefficients` row, which
/// is why this is two registry keys instead of a flag on one. **Do not collapse
/// them.**
fn health_differential(context: &MetricContext<'_>) -> f64 {
    let Some(participant) = context.participant() else {
        return 0.0;
    };
    if !participant.is_winner {
        return 0.0;
    }
    health_gap(context).unwrap_or(0.0)
}

/// The [`health_gap`] of a game this hero played, won or lost -- so the loser
/// scores the exact negative of what the winner scored, and a heavy defeat costs
/// a manager as much as a clean victory earns. The overkill a losing hero can
/// finish on (health below zero, per `MatchRule::LOSER_HAS_POSITIVE_HEALTH`)
/// widens the gap on both sides alike.
fn health_differential_two_way(context: &MetricContext<'_>) -> f64 {
    health_gap(context).unwrap_or(0.0)
}

/// A clean sweep: every opponent finished on zero health. Only awarded to a hero
/// that actually played and that had at least one opponent.
fn shutout(context: &MetricContext<'_>) -> f64 {
    if context.participant().is_none() {
        return 0.0;
    }
    let opponents = context.opponents();
    if opponents.is_empty() {
        return 0.0;
    }
    if opponents.iter().all(|o| o.health_remaining == 0) {
        1.0
    } else {
        0.0
    }
}

/// A per-metric truth table over hand-built matches -- a near-1:1 port of
/// `MatchMetricsTest`.
///
/// The fixtures mirror the shapes the seed data actually contains: a decided
/// match, the round-2 shutout, the round-3 game won on health with both heroes
/// alive, and (in `games`) the round-3 best-of-three. A metric that disagrees
/// with reality fails here rather than three layers up in the leaderboard.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_result::{
        BanResult, DraftedHeroResult, GameParticipantResult, GameResult, MatchParticipantResult,
        MatchResult,
    };
    use chrono::{DateTime, Utc};

    fn played() -> DateTime<Utc> {
        "2026-06-06T11:00:00Z".parse().unwrap()
    }

    fn game_participant(
        side: i32,
        hero_id: i64,
        hero_name: &str,
        health: i32,
        is_winner: bool,
    ) -> GameParticipantResult {
        GameParticipantResult {
            side,
            hero_id,
            hero_name: hero_name.into(),
            health_remaining: health,
            is_winner,
        }
    }

    /// A single-game match: one `GameResult` wrapping `game_participants`.
    ///
    /// Every side drafts the hero it fielded, since a recorded draft is complete
    /// (`MatchRule::PLAYED_HERO_NOT_DRAFTED`). `unplayed_picks` adds heroes side
    /// 0 drafted and never played -- the case `APPEARANCE` exists to catch.
    fn match_of(
        game_participants: Vec<GameParticipantResult>,
        bans: Vec<BanResult>,
        unplayed_picks: Vec<DraftedHeroResult>,
    ) -> MatchResult {
        let participants = game_participants
            .iter()
            .map(|participant| {
                let mut drafted_heroes = vec![DraftedHeroResult {
                    hero_id: participant.hero_id,
                    hero_name: participant.hero_name.clone(),
                }];
                if participant.side == 0 {
                    drafted_heroes.extend(unplayed_picks.iter().cloned());
                }
                MatchParticipantResult {
                    side: participant.side,
                    player_label: Some(format!("Player {}", participant.side)),
                    drafted_heroes,
                }
            })
            .collect();

        MatchResult {
            match_id: 6,
            tournament_id: 1,
            round: 2,
            played_at: played(),
            external_link: "https://example.com/match/metrics".into(),
            participants,
            games: vec![GameResult {
                game_id: 1,
                game_number: 1,
                map_id: 3,
                map_name: "Raptor Paddock".into(),
                participants: game_participants,
            }],
            bans,
        }
    }

    fn simple(game_participants: Vec<GameParticipantResult>) -> MatchResult {
        match_of(game_participants, Vec::new(), Vec::new())
    }

    fn ban(hero_id: i64, hero_name: &str, ban_type: BanType) -> BanResult {
        BanResult {
            hero_id,
            hero_name: hero_name.into(),
            ban_type,
            side: None,
        }
    }

    /// Match 6 of the seed: Bigfoot 11 beats Beowulf 0.
    fn shutout_match() -> MatchResult {
        match_of(
            vec![
                game_participant(0, 7, "Bigfoot", 11, true),
                game_participant(1, 11, "Beowulf", 0, false),
            ],
            vec![ban(8, "Sun Wukong", BanType::PreBan)],
            Vec::new(),
        )
    }

    /// Match 11 of the seed: Sherlock Holmes wins 7 to Dracula's 0.
    fn decisive_match() -> MatchResult {
        simple(vec![
            game_participant(0, 5, "Sherlock Holmes", 7, true),
            game_participant(1, 6, "Dracula", 0, false),
        ])
    }

    /// One of each ban category, to tell `SELF_BAN` and `OPPONENT_BAN` apart.
    fn ban_variety_match() -> MatchResult {
        match_of(
            vec![
                game_participant(0, 1, "Alice", 5, true),
                game_participant(1, 2, "Medusa", 0, false),
            ],
            vec![
                ban(20, "Invisible Man", BanType::SelfBan),
                ban(21, "Robin Hood", BanType::OpponentBan),
            ],
            Vec::new(),
        )
    }

    /// Alice takes it at 6, Medusa survives on `medusa_health`, Sinbad is out.
    fn three_way(medusa_health: i32) -> MatchResult {
        simple(vec![
            game_participant(0, 1, "Alice", 6, true),
            game_participant(1, 2, "Medusa", medusa_health, false),
            game_participant(2, 3, "Sinbad", 0, false),
        ])
    }

    /// The hero's played-or-banned context -- the per-game one for a hero that
    /// played a single-game match, the ban marker for one that did not. Its
    /// `Drafted` context is deliberately excluded: a drafted hero that played
    /// has both, and every per-game metric is measured on the `Played` one.
    fn context_for(match_result: &MatchResult, hero_id: i64) -> MetricContext<'_> {
        let mut found: Vec<MetricContext<'_>> = match_result
            .hero_contexts()
            .into_iter()
            .filter(|c| c.hero_id == hero_id && !matches!(c.role, HeroRole::Drafted))
            .collect();
        assert_eq!(
            found.len(),
            1,
            "no unique play-or-ban context for hero {hero_id}"
        );
        found.remove(0)
    }

    /// The hero's once-per-series `Drafted` context -- what `APPEARANCE` prices.
    fn draft_context_for(match_result: &MatchResult, hero_id: i64) -> MetricContext<'_> {
        let mut found: Vec<MetricContext<'_>> = match_result
            .hero_contexts()
            .into_iter()
            .filter(|c| c.hero_id == hero_id && matches!(c.role, HeroRole::Drafted))
            .collect();
        assert_eq!(found.len(), 1, "no draft context for hero {hero_id}");
        found.remove(0)
    }

    fn played_contexts(match_result: &MatchResult, hero_id: i64) -> Vec<MetricContext<'_>> {
        match_result
            .hero_contexts()
            .into_iter()
            .filter(|c| c.hero_id == hero_id && matches!(c.role, HeroRole::Played { .. }))
            .collect()
    }

    fn measure(metric: &str, context: &MetricContext<'_>) -> f64 {
        let extractor = get(metric).unwrap_or_else(|| panic!("no extractor for {metric}"));
        extractor(context)
    }

    mod registry {
        use super::*;

        #[test]
        fn every_advertised_metric_resolves_to_an_extractor() {
            assert_eq!(
                known().collect::<Vec<_>>(),
                vec![
                    "APPEARANCE",
                    "SELF_BAN",
                    "OPPONENT_BAN",
                    "WIN",
                    "LOSS",
                    "HEALTH_REMAINING",
                    "HEALTH_DIFFERENTIAL",
                    "HEALTH_DIFFERENTIAL_TWO_WAY",
                    "SHUTOUT",
                ]
            );
            for metric in known() {
                assert!(get(metric).is_some(), "{metric} is advertised but unpriced");
            }
        }

        #[test]
        fn lookup_is_case_and_whitespace_insensitive() {
            assert!(get(" win ").is_some());
            assert!(get("Health_Remaining").is_some());
        }

        #[test]
        fn unknown_catches_a_typo_and_leaves_the_real_keys_alone() {
            assert_eq!(
                unknown(["WIN", "HEALTH_REMAINNG", "SELF_BAN", "CROWD_FAVOURITE"]),
                vec!["HEALTH_REMAINNG".to_string(), "CROWD_FAVOURITE".to_string()]
            );
        }

        /// Pricing `DRAW` would be pricing something `MatchResultPolicy`
        /// rejects, so it is warned about like any other metric this build
        /// cannot measure.
        #[test]
        fn draw_is_not_a_metric_because_a_game_with_no_winner_is_not_recordable() {
            assert!(get("DRAW").is_none());
            assert_eq!(unknown(["WIN", "DRAW", "LOSS"]), vec!["DRAW".to_string()]);
        }

        #[test]
        fn an_unimplemented_metric_resolves_to_nothing_rather_than_panicking() {
            assert!(get("CROWD_FAVOURITE").is_none());
        }

        #[test]
        fn labels_are_title_cased_for_the_leaderboard_header() {
            assert_eq!(label("HEALTH_REMAINING"), "Health Remaining");
            assert_eq!(label("win"), "Win");
            assert_eq!(label("HEALTH_DIFFERENTIAL"), "Health Differential");
        }
    }

    mod outcomes {
        use super::*;

        #[test]
        fn the_winner_takes_win_and_not_loss() {
            let m = shutout_match();
            let bigfoot = context_for(&m, 7);

            assert_eq!(measure("WIN", &bigfoot), 1.0);
            assert_eq!(measure("LOSS", &bigfoot), 0.0);
        }

        #[test]
        fn the_defeated_hero_takes_loss_only() {
            let m = shutout_match();
            let beowulf = context_for(&m, 11);

            assert_eq!(measure("WIN", &beowulf), 0.0);
            assert_eq!(measure("LOSS", &beowulf), 1.0);
        }

        #[test]
        fn win_and_loss_are_exhaustive_within_a_decided_game() {
            let m = decisive_match();
            let sherlock = context_for(&m, 5);
            let dracula = context_for(&m, 6);

            assert_eq!(measure("WIN", &sherlock), 1.0);
            assert_eq!(measure("LOSS", &sherlock), 0.0);
            assert_eq!(measure("WIN", &dracula), 0.0);
            assert_eq!(measure("LOSS", &dracula), 1.0);
        }
    }

    mod health {
        use super::*;

        #[test]
        fn health_remaining_is_the_heros_own_end_of_game_health() {
            let shutout = shutout_match();
            let decisive = decisive_match();

            assert_eq!(measure("HEALTH_REMAINING", &context_for(&shutout, 7)), 11.0);
            assert_eq!(measure("HEALTH_REMAINING", &context_for(&shutout, 11)), 0.0);
            assert_eq!(measure("HEALTH_REMAINING", &context_for(&decisive, 6)), 0.0);
        }

        #[test]
        fn health_differential_is_win_gated_so_only_the_winner_scores_it() {
            let m = decisive_match();

            assert_eq!(measure("HEALTH_DIFFERENTIAL", &context_for(&m, 5)), 7.0);
            assert_eq!(
                measure("HEALTH_DIFFERENTIAL", &context_for(&m, 6)),
                0.0,
                "the losing side has no differential to price"
            );
        }

        #[test]
        fn health_differential_measures_the_winner_against_the_healthiest_opponent() {
            let m = three_way(4);

            assert_eq!(measure("HEALTH_DIFFERENTIAL", &context_for(&m, 1)), 2.0);
            assert_eq!(measure("HEALTH_DIFFERENTIAL", &context_for(&m, 2)), 0.0);
            assert_eq!(measure("HEALTH_DIFFERENTIAL", &context_for(&m, 3)), 0.0);
        }

        #[test]
        fn health_differential_two_way_prices_the_loser_at_the_exact_negative_of_the_winner() {
            let m = decisive_match();
            let sherlock = context_for(&m, 5);
            let dracula = context_for(&m, 6);

            assert_eq!(measure("HEALTH_DIFFERENTIAL_TWO_WAY", &sherlock), 7.0);
            assert_eq!(measure("HEALTH_DIFFERENTIAL_TWO_WAY", &dracula), -7.0);

            // The win-gated original is the whole difference between the two
            // keys: same winner, but nothing at all for the side that lost.
            assert_eq!(measure("HEALTH_DIFFERENTIAL", &sherlock), 7.0);
            assert_eq!(measure("HEALTH_DIFFERENTIAL", &dracula), 0.0);
        }

        #[test]
        fn health_differential_two_way_measures_every_side_against_the_healthiest_opponent() {
            let m = three_way(4);

            // Past two sides the pairing stops being zero-sum, by design: each
            // hero is priced against the best of the rest, so both losers
            // measure against Alice's 6 rather than against each other.
            assert_eq!(
                measure("HEALTH_DIFFERENTIAL_TWO_WAY", &context_for(&m, 1)),
                2.0
            );
            assert_eq!(
                measure("HEALTH_DIFFERENTIAL_TWO_WAY", &context_for(&m, 2)),
                -2.0
            );
            assert_eq!(
                measure("HEALTH_DIFFERENTIAL_TWO_WAY", &context_for(&m, 3)),
                -6.0
            );
        }

        #[test]
        fn an_overkill_finish_widens_the_two_way_differential_on_both_sides_alike() {
            let overkill = simple(vec![
                game_participant(0, 7, "Bigfoot", 11, true),
                game_participant(1, 11, "Beowulf", -3, false),
            ]);

            assert_eq!(
                measure("HEALTH_DIFFERENTIAL_TWO_WAY", &context_for(&overkill, 7)),
                14.0
            );
            assert_eq!(
                measure("HEALTH_DIFFERENTIAL_TWO_WAY", &context_for(&overkill, 11)),
                -14.0
            );
        }
    }

    mod shutout_metric {
        use super::*;

        #[test]
        fn a_shutout_needs_every_opponent_on_zero() {
            let m = shutout_match();
            assert_eq!(measure("SHUTOUT", &context_for(&m, 7)), 1.0);
        }

        #[test]
        fn the_shut_out_hero_does_not_itself_earn_a_shutout() {
            let m = shutout_match();
            assert_eq!(measure("SHUTOUT", &context_for(&m, 11)), 0.0);
        }

        #[test]
        fn a_win_with_the_opponent_still_alive_is_no_shutout() {
            let narrow = simple(vec![
                game_participant(0, 4, "Medusa", 8, true),
                game_participant(1, 8, "Sun Wukong", 2, false),
            ]);

            assert_eq!(measure("SHUTOUT", &context_for(&narrow, 4)), 0.0);
        }

        #[test]
        fn one_surviving_opponent_out_of_two_denies_the_shutout() {
            let m = three_way(1);

            assert_eq!(measure("SHUTOUT", &context_for(&m, 1)), 0.0);
        }
    }

    mod bans {
        use super::*;

        #[test]
        fn a_pre_banned_hero_earns_neither_self_ban_nor_opponent_ban() {
            let m = shutout_match();
            let sun_wukong = context_for(&m, 8);

            for metric in known() {
                assert_eq!(
                    measure(metric, &sun_wukong),
                    0.0,
                    "a PRE_BAN is priced by nothing, but {metric} scored"
                );
            }
        }

        #[test]
        fn a_self_banned_hero_earns_self_ban_and_nothing_else() {
            let m = ban_variety_match();
            let invisible_man = context_for(&m, 20);

            assert_eq!(measure("SELF_BAN", &invisible_man), 1.0);
            assert_eq!(measure("OPPONENT_BAN", &invisible_man), 0.0);
            assert_eq!(measure("APPEARANCE", &invisible_man), 0.0);
        }

        #[test]
        fn an_opponent_banned_hero_earns_opponent_ban_and_nothing_else() {
            let m = ban_variety_match();
            let robin_hood = context_for(&m, 21);

            assert_eq!(measure("OPPONENT_BAN", &robin_hood), 1.0);
            assert_eq!(measure("SELF_BAN", &robin_hood), 0.0);
            assert_eq!(measure("APPEARANCE", &robin_hood), 0.0);
        }

        #[test]
        fn a_hero_that_played_earns_appearance_off_its_draft_and_never_a_ban_metric() {
            let m = shutout_match();
            let bigfoot_draft = draft_context_for(&m, 7);
            let bigfoot_played = context_for(&m, 7);

            assert_eq!(measure("APPEARANCE", &bigfoot_draft), 1.0);
            assert_eq!(
                measure("APPEARANCE", &bigfoot_played),
                0.0,
                "appearance is a draft fact, not a game one"
            );
            assert_eq!(measure("SELF_BAN", &bigfoot_draft), 0.0);
            assert_eq!(measure("OPPONENT_BAN", &bigfoot_draft), 0.0);
        }

        #[test]
        fn hero_contexts_covers_every_hero_a_single_game_match_touched() {
            let m = shutout_match();
            let contexts = m.hero_contexts();

            // Both heroes played and were drafted; Sun Wukong was banned out.
            assert_eq!(
                contexts.iter().map(|c| c.hero_id).collect::<Vec<_>>(),
                vec![7, 11, 7, 11, 8]
            );
            let mut distinct: Vec<i64> = Vec::new();
            for context in &contexts {
                if !distinct.contains(&context.hero_id) {
                    distinct.push(context.hero_id);
                }
            }
            assert_eq!(distinct.len(), 3);
            assert!(matches!(context_for(&m, 8).role, HeroRole::Banned));
        }

        #[test]
        fn a_banned_heros_role_is_a_bare_marker_its_category_lives_on_the_ban() {
            let m = shutout_match();

            assert_eq!(context_for(&m, 8).role, HeroRole::Banned);
            let struck = m.bans.iter().find(|b| b.hero_id == 8).unwrap();
            assert_eq!(struck.ban_type, BanType::PreBan);
        }
    }

    mod draft {
        use super::*;

        /// Match 6 again, but side 0 also drafted a hero it never fielded.
        fn benched_match() -> MatchResult {
            match_of(
                vec![
                    game_participant(0, 7, "Bigfoot", 11, true),
                    game_participant(1, 11, "Beowulf", 0, false),
                ],
                vec![ban(8, "Sun Wukong", BanType::PreBan)],
                vec![DraftedHeroResult {
                    hero_id: 30,
                    hero_name: "Tomoe Gozen".into(),
                }],
            )
        }

        #[test]
        fn a_hero_drafted_and_never_fielded_still_earns_appearance() {
            let m = benched_match();
            assert_eq!(measure("APPEARANCE", &draft_context_for(&m, 30)), 1.0);
        }

        #[test]
        fn a_hero_drafted_and_never_fielded_earns_nothing_a_game_would_have_given_it() {
            let m = benched_match();
            let tomoe = draft_context_for(&m, 30);

            for metric in known().filter(|name| *name != "APPEARANCE") {
                assert_eq!(
                    measure(metric, &tomoe),
                    0.0,
                    "an unfielded pick earns only APPEARANCE, but {metric} scored"
                );
            }
        }

        #[test]
        fn an_unfielded_pick_yields_one_context_and_it_is_not_a_game() {
            let m = benched_match();
            let contexts: Vec<MetricContext<'_>> = m
                .hero_contexts()
                .into_iter()
                .filter(|c| c.hero_id == 30)
                .collect();

            assert_eq!(contexts.len(), 1);
            assert_eq!(contexts[0].role, HeroRole::Drafted);
            assert!(
                contexts[0].participant().is_none(),
                "a hero that never played has no participant row"
            );
        }

        #[test]
        fn a_banned_hero_is_never_also_drafted_so_it_earns_no_appearance() {
            let m = benched_match();

            assert_eq!(measure("APPEARANCE", &context_for(&m, 8)), 0.0);
            assert!(
                !m.hero_contexts()
                    .iter()
                    .any(|c| c.hero_id == 8 && matches!(c.role, HeroRole::Drafted))
            );
        }
    }

    mod games {
        use super::*;

        /// The seed's Bo3 (match 13) with only its first two games played:
        /// Medusa takes game 1, Achilles takes game 2 back. The point of the
        /// fixture is that the same two heroes appear twice with opposite
        /// outcomes, so a metric folded per *series* instead of per *game*
        /// cannot pass.
        fn best_of_two_so_far() -> MatchResult {
            MatchResult {
                match_id: 13,
                tournament_id: 1,
                round: 3,
                played_at: played(),
                external_link: "https://challonge.com/example-bo3-decider".into(),
                participants: vec![
                    MatchParticipantResult {
                        side: 0,
                        player_label: Some("Rina Okafor".into()),
                        drafted_heroes: vec![DraftedHeroResult {
                            hero_id: 4,
                            hero_name: "Medusa".into(),
                        }],
                    },
                    MatchParticipantResult {
                        side: 1,
                        player_label: Some("Dmitri Kovac".into()),
                        drafted_heroes: vec![DraftedHeroResult {
                            hero_id: 20,
                            hero_name: "Achilles".into(),
                        }],
                    },
                ],
                games: vec![
                    GameResult {
                        game_id: 101,
                        game_number: 1,
                        map_id: 2,
                        map_name: "Sherwood Forest".into(),
                        participants: vec![
                            game_participant(0, 4, "Medusa", 6, true),
                            game_participant(1, 20, "Achilles", 0, false),
                        ],
                    },
                    GameResult {
                        game_id: 102,
                        game_number: 2,
                        map_id: 3,
                        map_name: "Raptor Paddock".into(),
                        participants: vec![
                            game_participant(0, 4, "Medusa", 0, false),
                            game_participant(1, 20, "Achilles", 5, true),
                        ],
                    },
                ],
                bans: Vec::new(),
            }
        }

        /// Game 3, added to make the series a genuine Bo3.
        fn decider() -> GameResult {
            GameResult {
                game_id: 103,
                game_number: 3,
                map_id: 1,
                map_name: "Baskerville Manor".into(),
                participants: vec![
                    game_participant(0, 4, "Medusa", 3, true),
                    game_participant(1, 20, "Achilles", 0, false),
                ],
            }
        }

        #[test]
        fn a_hero_in_two_games_yields_two_played_contexts_each_scoring_its_own_game() {
            let m = best_of_two_so_far();
            let medusa = played_contexts(&m, 4);

            assert_eq!(medusa.len(), 2);
            assert_eq!(measure("HEALTH_REMAINING", &medusa[0]), 6.0);
            assert_eq!(measure("HEALTH_REMAINING", &medusa[1]), 0.0);
        }

        #[test]
        fn win_and_loss_are_scoped_per_game_not_per_series() {
            let m = best_of_two_so_far();
            let medusa = played_contexts(&m, 4);
            let achilles = played_contexts(&m, 20);

            // Game 1: Medusa took it, Achilles lost it.
            assert_eq!(measure("WIN", &medusa[0]), 1.0);
            assert_eq!(measure("LOSS", &medusa[0]), 0.0);
            assert_eq!(measure("WIN", &achilles[0]), 0.0);
            assert_eq!(measure("LOSS", &achilles[0]), 1.0);

            // Game 2: the other way round, in the same series. Each hero ends
            // the series holding one WIN and one LOSS rather than a single
            // verdict.
            assert_eq!(measure("WIN", &medusa[1]), 0.0);
            assert_eq!(measure("LOSS", &medusa[1]), 1.0);
            assert_eq!(measure("WIN", &achilles[1]), 1.0);
            assert_eq!(measure("LOSS", &achilles[1]), 0.0);
        }

        #[test]
        fn a_banned_hero_yields_one_banned_context_however_many_games_the_series_has() {
            let mut m = best_of_two_so_far();
            m.games.push(decider());
            m.bans = vec![ban(6, "Bruce Lee", BanType::PreBan)];

            let ban_contexts: Vec<MetricContext<'_>> = m
                .hero_contexts()
                .into_iter()
                .filter(|c| c.hero_id == 6)
                .collect();

            assert_eq!(
                ban_contexts.len(),
                1,
                "a ban is struck once, before any game -- it must not multiply by game count"
            );
            assert!(matches!(ban_contexts[0].role, HeroRole::Banned));
        }

        #[test]
        fn a_hero_that_played_every_game_of_a_bo3_is_still_drafted_once() {
            let mut m = best_of_two_so_far();
            m.games.push(decider());

            assert_eq!(played_contexts(&m, 4).len(), 3);
            let draft_contexts: Vec<MetricContext<'_>> = m
                .hero_contexts()
                .into_iter()
                .filter(|c| c.hero_id == 4 && matches!(c.role, HeroRole::Drafted))
                .collect();
            assert_eq!(
                draft_contexts.len(),
                1,
                "a draft happens once for the series, like a ban"
            );
            assert_eq!(measure("APPEARANCE", &draft_contexts[0]), 1.0);
        }
    }
}
