//! The hero performance fold: recorded matches plus rules plus the hero pool,
//! in; one ranked table per scoring criterion, out.
//!
//! This is the same input the leaderboard folds, grouped by *hero* rather than
//! by a manager's holdings. A hero's points here are what it earned in every
//! match of the tournament, whoever held it at the time -- so there are no
//! rosters and no swap log involved, and the sum a manager sees on the
//! leaderboard is always the part of these numbers their holdings covered.
//!
//! **Nothing here stores a point total**, for the same reason the leaderboard
//! does not: coefficients are retuned with a bare UPDATE. See AGENTS.md's
//! "Nothing writes points".

use crate::match_result::MatchResult;
use crate::rounding::round2;
use crate::scoring_engine::{self, ScoringRules};
use crate::standings::{competition_rank, metric_columns};
use indexmap::IndexMap;
use serde::Serialize;

/// One hero in the tournament's pool, with *this* tournament's price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolHero {
    pub hero_id: i64,
    pub name: String,
    pub image_url: Option<String>,
    pub cost: i32,
}

/// One hero's place in one table.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeroRankRow {
    pub rank: i32,
    pub hero_id: i64,
    pub hero_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    /// This tournament's price -- every row is a pool hero, so there always is one.
    pub cost: i32,
    pub points: f64,
}

/// One scoring criterion's table, ranking every hero on that metric alone.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeroCategory {
    pub metric: String,
    pub label: String,
    pub coefficient: f64,
    pub rows: Vec<HeroRankRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeroStatsBoard {
    pub tournament_id: i64,
    pub rule_set_name: String,
    /// `max(round)` over recorded matches, exactly as on the leaderboard.
    pub current_round: i32,
    /// Every hero ranked by total points across all criteria.
    pub overall: Vec<HeroRankRow>,
    /// One per scored metric, in the rule set's own column order. Unknown
    /// metrics never get a table, just as they never get a leaderboard column.
    pub categories: Vec<HeroCategory>,
}

/// Everything one pool hero earned, before ranking.
struct HeroTally<'a> {
    hero: &'a PoolHero,
    breakdown: IndexMap<String, f64>,
}

impl HeroTally<'_> {
    fn row(&self, points: f64) -> HeroRankRow {
        HeroRankRow {
            // Replaced once the table is ordered.
            rank: 0,
            hero_id: self.hero.hero_id,
            hero_name: self.hero.name.clone(),
            image_url: self.hero.image_url.clone(),
            cost: self.hero.cost,
            points,
        }
    }
}

/// Folds recorded matches into per-hero, per-criterion rankings.
///
/// The hero set is exactly the pool: these tables exist to inform a draft, and
/// only a pool hero can be drafted. A hero nobody has fielded yet ranks on 0
/// in every table, which is itself worth knowing. A hero a result names from
/// outside the pool -- one dropped from the pool after it scored, say -- is
/// not listed at all; its contexts are priced by nobody here, exactly as no
/// roster can hold it.
///
/// Each context is priced by [`scoring_engine::breakdown`], already rounded
/// per contribution, and summed -- the same arithmetic as
/// [`crate::standings::board`], so a manager holding one hero all tournament
/// shows exactly that hero's overall total.
pub fn board(
    tournament_id: i64,
    matches: &[MatchResult],
    rules: &ScoringRules,
    pool: &[PoolHero],
) -> HeroStatsBoard {
    let current_round = matches.iter().map(|m| m.round).max().unwrap_or(0);

    // Dense: every hero carries every scored metric, zeros included, so each
    // table ranks the same full set of heroes.
    let zeros = || -> IndexMap<String, f64> {
        rules
            .scored_metrics()
            .iter()
            .map(|metric| (metric.clone(), 0.0))
            .collect()
    };

    let mut tallies: IndexMap<i64, HeroTally> = pool
        .iter()
        .map(|hero| {
            (
                hero.hero_id,
                HeroTally {
                    hero,
                    breakdown: zeros(),
                },
            )
        })
        .collect();

    for match_result in matches {
        for context in match_result.hero_contexts() {
            let Some(tally) = tallies.get_mut(&context.hero_id) else {
                continue;
            };
            for (metric, points) in scoring_engine::breakdown(&context, rules) {
                *tally.breakdown.entry(metric).or_insert(0.0) += points;
            }
        }
    }

    for tally in tallies.values_mut() {
        for points in tally.breakdown.values_mut() {
            *points = round2(*points);
        }
    }

    let overall = ranked(
        tallies
            .values()
            .map(|tally| tally.row(round2(tally.breakdown.values().sum())))
            .collect(),
    );

    let categories = metric_columns(rules)
        .into_iter()
        .map(|column| HeroCategory {
            rows: ranked(
                tallies
                    .values()
                    .map(|tally| tally.row(tally.breakdown[&column.metric]))
                    .collect(),
            ),
            metric: column.metric,
            label: column.label,
            coefficient: column.coefficient,
        })
        .collect();

    HeroStatsBoard {
        tournament_id,
        rule_set_name: rules.name.clone(),
        current_round,
        overall,
        categories,
    }
}

/// Points descending, standard competition ranking, ties listed by name --
/// the leaderboard's own rule, applied to heroes.
///
/// With a negative coefficient (a `LOSS` penalty, say) the most-penalised
/// heroes sink to the bottom, which is the reading a manager wants: the top of
/// every table is where the points come from.
fn ranked(mut rows: Vec<HeroRankRow>) -> Vec<HeroRankRow> {
    competition_rank(
        &mut rows,
        |row| row.points,
        |row| &row.hero_name,
        |row, rank| row.rank = rank,
    );
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_result::{
        BanResult, BanType, DraftedHeroResult, GameParticipantResult, GameResult,
        MatchParticipantResult,
    };
    use crate::standings::{self, EntryRoster, RosterHero};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn rules(weights: &[(&str, &str)]) -> ScoringRules {
        let coefficients = weights
            .iter()
            .map(|(metric, weight)| ((*metric).to_string(), Decimal::from_str(weight).unwrap()))
            .collect();
        ScoringRules::new(1, "Season 2026 Standard", coefficients)
    }

    /// The seed's weights, `CROWD_FAVOURITE` included -- it is deliberately
    /// unimplemented, so it must never become a table.
    fn standard() -> ScoringRules {
        rules(&[
            ("WIN", "10.0000"),
            ("HEALTH_REMAINING", "0.7500"),
            ("APPEARANCE", "1.0000"),
            ("OPPONENT_BAN", "2.0000"),
            ("CROWD_FAVOURITE", "5.0000"),
        ])
    }

    fn hero(hero_id: i64, name: &str) -> DraftedHeroResult {
        DraftedHeroResult {
            hero_id,
            hero_name: name.into(),
        }
    }

    fn played(
        side: i32,
        hero_id: i64,
        name: &str,
        health: i32,
        is_winner: bool,
    ) -> GameParticipantResult {
        GameParticipantResult {
            side,
            hero_id,
            hero_name: name.into(),
            health_remaining: health,
            is_winner,
        }
    }

    fn game(
        game_id: i64,
        game_number: i32,
        participants: Vec<GameParticipantResult>,
    ) -> GameResult {
        GameResult {
            game_id,
            game_number,
            map_id: 3,
            map_name: "Raptor Paddock".into(),
            participants,
        }
    }

    /// Bigfoot (7) beats Beowulf (11) on 11 health; Sun Wukong (8) is struck
    /// by an opponent ban; Alice (9) is drafted and never fielded.
    fn one_game_match() -> MatchResult {
        MatchResult {
            match_id: 6,
            tournament_id: 1,
            round: 2,
            played_at: "2026-06-06T11:00:00Z".parse().unwrap(),
            external_link: "https://example.com/match/6".into(),
            participants: vec![
                MatchParticipantResult {
                    side: 0,
                    player_label: Some("Aurelie Blanc".into()),
                    drafted_heroes: vec![hero(7, "Bigfoot"), hero(9, "Alice")],
                },
                MatchParticipantResult {
                    side: 1,
                    player_label: Some("Miles Ashworth".into()),
                    drafted_heroes: vec![hero(11, "Beowulf")],
                },
            ],
            games: vec![game(
                1,
                1,
                vec![
                    played(0, 7, "Bigfoot", 11, true),
                    played(1, 11, "Beowulf", 0, false),
                ],
            )],
            bans: vec![BanResult {
                hero_id: 8,
                hero_name: "Sun Wukong".into(),
                ban_type: BanType::OpponentBan,
                side: Some(0),
            }],
        }
    }

    /// A Bo2 in which Bigfoot wins both games.
    fn two_game_match() -> MatchResult {
        MatchResult {
            games: vec![
                game(
                    1,
                    1,
                    vec![
                        played(0, 7, "Bigfoot", 11, true),
                        played(1, 11, "Beowulf", 0, false),
                    ],
                ),
                game(
                    2,
                    2,
                    vec![
                        played(0, 7, "Bigfoot", 4, true),
                        played(1, 11, "Beowulf", -2, false),
                    ],
                ),
            ],
            ..one_game_match()
        }
    }

    fn pool_hero(hero_id: i64, name: &str, cost: i32) -> PoolHero {
        PoolHero {
            hero_id,
            name: name.into(),
            image_url: None,
            cost,
        }
    }

    /// Every hero in the fixture match, plus Medusa (12), who never appears.
    fn pool() -> Vec<PoolHero> {
        vec![
            pool_hero(7, "Bigfoot", 2500),
            pool_hero(8, "Sun Wukong", 2200),
            pool_hero(9, "Alice", 1200),
            pool_hero(11, "Beowulf", 2400),
            pool_hero(12, "Medusa", 1800),
        ]
    }

    fn category<'a>(board: &'a HeroStatsBoard, metric: &str) -> &'a HeroCategory {
        board
            .categories
            .iter()
            .find(|c| c.metric == metric)
            .unwrap_or_else(|| panic!("no {metric} table"))
    }

    fn table(rows: &[HeroRankRow]) -> Vec<(i32, &str, f64)> {
        rows.iter()
            .map(|row| (row.rank, row.hero_name.as_str(), row.points))
            .collect()
    }

    #[test]
    fn one_table_per_scored_metric_in_the_rule_sets_order() {
        let board = board(1, &[one_game_match()], &standard(), &pool());

        let metrics: Vec<&str> = board.categories.iter().map(|c| c.metric.as_str()).collect();
        assert_eq!(
            metrics,
            vec!["WIN", "HEALTH_REMAINING", "APPEARANCE", "OPPONENT_BAN"],
            "CROWD_FAVOURITE is weighted but unimplemented, so it gets no table"
        );
        assert_eq!(board.categories[0].label, "Win");
        assert_eq!(board.categories[0].coefficient, 10.0);
        assert_eq!(board.rule_set_name, "Season 2026 Standard");
        assert_eq!(board.current_round, 2);
    }

    #[test]
    fn each_table_ranks_every_pool_hero_on_that_metric_alone() {
        let board = board(1, &[one_game_match()], &standard(), &pool());

        // Four heroes tie on zero wins: they share rank 2 and are listed by
        // name.
        assert_eq!(
            table(&category(&board, "WIN").rows),
            vec![
                (1, "Bigfoot", 10.0),
                (2, "Alice", 0.0),
                (2, "Beowulf", 0.0),
                (2, "Medusa", 0.0),
                (2, "Sun Wukong", 0.0),
            ]
        );
        // Drafted-and-unfielded Alice earns an appearance; banned Sun Wukong
        // does not; nobody drafted Medusa. The three-way tie consumes ranks
        // 2 and 3.
        assert_eq!(
            table(&category(&board, "APPEARANCE").rows),
            vec![
                (1, "Alice", 1.0),
                (1, "Beowulf", 1.0),
                (1, "Bigfoot", 1.0),
                (4, "Medusa", 0.0),
                (4, "Sun Wukong", 0.0),
            ]
        );
        assert_eq!(
            table(&category(&board, "OPPONENT_BAN").rows)[0],
            (1, "Sun Wukong", 2.0)
        );
    }

    #[test]
    fn the_overall_table_ranks_the_sum_of_every_category() {
        let board = board(1, &[one_game_match()], &standard(), &pool());

        assert_eq!(
            table(&board.overall),
            vec![
                (1, "Bigfoot", 19.25), // 10 + 8.25 + 1
                (2, "Sun Wukong", 2.0),
                (3, "Alice", 1.0),
                (3, "Beowulf", 1.0),
                (5, "Medusa", 0.0),
            ]
        );

        for row in &board.overall {
            let parts: f64 = board
                .categories
                .iter()
                .flat_map(|c| &c.rows)
                .filter(|r| r.hero_id == row.hero_id)
                .map(|r| r.points)
                .sum();
            assert_eq!(round2(parts), row.points, "{}", row.hero_name);
        }
    }

    #[test]
    fn a_hero_nobody_has_fielded_is_still_ranked_on_zero() {
        let board = board(1, &[], &standard(), &pool());

        assert_eq!(board.current_round, 0);
        assert_eq!(board.overall.len(), 5);
        assert!(board.overall.iter().all(|r| r.points == 0.0 && r.rank == 1));
        for category in &board.categories {
            assert_eq!(category.rows.len(), 5, "{}", category.metric);
        }
    }

    #[test]
    fn a_hero_outside_the_pool_is_never_listed_even_when_it_scored() {
        // Bigfoot wins the match, but a pool without him is a pool nobody can
        // draft him from: he is not ranked, and nobody else inherits his rank.
        let without_bigfoot: Vec<PoolHero> =
            pool().into_iter().filter(|h| h.hero_id != 7).collect();

        let board = board(1, &[one_game_match()], &standard(), &without_bigfoot);

        assert!(board.overall.iter().all(|r| r.hero_id != 7));
        for category in &board.categories {
            assert_eq!(category.rows.len(), 4, "{}", category.metric);
            assert!(category.rows.iter().all(|r| r.hero_id != 7));
        }
        assert_eq!(table(&category(&board, "WIN").rows)[0], (1, "Alice", 0.0));
    }

    #[test]
    fn a_hero_that_played_twice_scores_each_game_but_appears_once() {
        let board = board(1, &[two_game_match()], &standard(), &pool());

        assert_eq!(category(&board, "WIN").rows[0].points, 20.0);
        assert_eq!(category(&board, "HEALTH_REMAINING").rows[0].points, 11.25);
        let appearance = category(&board, "APPEARANCE");
        let bigfoot = appearance.rows.iter().find(|r| r.hero_id == 7).unwrap();
        assert_eq!(bigfoot.points, 1.0);
    }

    #[test]
    fn a_penalty_ranks_the_most_penalised_hero_last() {
        let penalised = rules(&[("LOSS", "-6.0000")]);
        let board = board(1, &[two_game_match()], &penalised, &pool());

        let loss = category(&board, "LOSS");
        let last = loss.rows.last().unwrap();
        assert_eq!(last.hero_name, "Beowulf");
        assert_eq!(last.points, -12.0);
        assert_eq!(loss.rows[0].points, 0.0);
    }

    #[test]
    fn a_single_hero_roster_scores_exactly_that_heros_overall_total() {
        // The two folds share their arithmetic; a manager who held one hero
        // all tournament is that hero's overall line.
        let matches = [one_game_match(), two_game_match()];
        let heroes = board(1, &matches, &standard(), &pool());
        let entry = EntryRoster {
            entry_id: 1,
            manager_id: 1,
            handle: "Solo".into(),
            display_name: "Solo".into(),
            credit_grant: 10_000,
            heroes: vec![RosterHero {
                slot_index: 0,
                hero_id: 7,
                name: "Bigfoot".into(),
                cost: 2500,
            }],
            swaps: Vec::new(),
        };
        let managers = standings::board(1, &matches, &standard(), &[entry]);

        let bigfoot = heroes.overall.iter().find(|r| r.hero_id == 7).unwrap();
        assert_eq!(bigfoot.points, managers.rows[0].total_points);
    }

    #[test]
    fn a_rule_set_that_scores_nothing_leaves_only_an_all_zero_overall() {
        let board = board(1, &[one_game_match()], &ScoringRules::none(), &pool());

        assert!(board.categories.is_empty());
        assert_eq!(board.overall.len(), 5);
        assert!(board.overall.iter().all(|r| r.points == 0.0));
    }

    #[test]
    fn the_board_serializes_under_camel_case_and_omits_an_absent_image() {
        let mut heroes = pool();
        heroes[0].image_url = Some("https://example.com/bigfoot.png".into());

        let board = board(1, &[one_game_match()], &standard(), &heroes);
        let json = serde_json::to_value(&board).unwrap();

        assert_eq!(json["tournamentId"], 1);
        assert_eq!(json["ruleSetName"], "Season 2026 Standard");
        assert_eq!(json["currentRound"], 2);
        assert_eq!(json["overall"][0]["heroName"], "Bigfoot");
        assert_eq!(json["overall"][0]["heroId"], 7);
        assert_eq!(json["overall"][0]["cost"], 2500);
        assert_eq!(
            json["overall"][0]["imageUrl"],
            "https://example.com/bigfoot.png"
        );
        assert_eq!(json["categories"][0]["metric"], "WIN");
        assert_eq!(json["categories"][0]["rows"][0]["points"], 10.0);

        // No image on file: the field is absent rather than null.
        let beowulf = json["overall"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["heroId"] == 11)
            .unwrap();
        assert_eq!(beowulf["cost"], 2400);
        assert!(beowulf.get("imageUrl").is_none(), "{beowulf}");
    }
}
