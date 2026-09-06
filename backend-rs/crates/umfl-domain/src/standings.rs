//! The leaderboard fold: recorded matches plus rosters plus rules, in;
//! a ranked board and a ticker, out.
//!
//! Only the *arithmetic* lives here, as plain functions over plain data: the
//! ranking, the dense/sparse split, the drafted-context banking and the
//! rounding are all testable this way with no Postgres involved. The server's
//! `standings::service` opens the snapshot, reads the rules, the cached matches
//! and the rosters, and calls [`board`] / [`ticker`].
//!
//! **Nothing here stores a point total.** Points are derived on every read
//! because coefficients and hero costs are mutable reference data retuned with
//! a bare UPDATE -- a stored total would be a cache with no invalidation
//! signal. What *is* cached is this fold's input, the assembled match list,
//! which has exactly one writer. See AGENTS.md's "Nothing writes points".

use crate::match_metrics;
use crate::match_result::{HeroRole, MatchResult};
use crate::rounding::round2;
use crate::scoring_engine::{self, ScoringRules};
use crate::time::java_instant;
use chrono::{DateTime, Utc};
use indexmap::{IndexMap, IndexSet};
use serde::Serialize;

/// One hero on a roster, with the price *this* tournament charges for it.
///
/// There is no cost snapshot: the price is joined live, so re-pricing a hero
/// re-prices every unlocked roster holding it. See AGENTS.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterHero {
    pub slot_index: i32,
    pub hero_id: i64,
    pub name: String,
    pub cost: i32,
}

/// One hero exchanged for another, in one round, by one entry.
///
/// A row of `roster_swaps`, ordered by `(round, id)` -- two swaps in the same
/// round still have to be replayed in the order they were made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RosterSwap {
    pub round: i32,
    pub hero_out_id: i64,
    pub hero_in_id: i64,
}

/// A stretch of rounds over which one entry held one hero.
///
/// `until_round` is exclusive and `None` while the hero is still held. A hero
/// traded away and later re-acquired yields *two* holdings rather than one
/// widened span, which is why this is a list rather than a pair of columns on
/// [`RosterHero`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holding {
    pub hero_id: i64,
    pub from_round: i32,
    pub until_round: Option<i32>,
}

impl Holding {
    /// Whether this holding covers the round a match was played in.
    fn covers(&self, round: i32) -> bool {
        round >= self.from_round && self.until_round.is_none_or(|until| round < until)
    }
}

/// One entry's roster, as the leaderboard needs it. Points are added by
/// [`board`].
///
/// An entry with no picks yet is still an entry and still belongs on the board,
/// which is why the query behind this is a **left** join onto `entry_slots` and
/// why `heroes` may legitimately be empty.
///
/// `heroes` is the roster held **now**; `swaps` is how it got that way. The two
/// together are what let [`board`] price a match against the roster its round
/// was actually played with -- see [`EntryRoster::holdings`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRoster {
    pub entry_id: i64,
    pub manager_id: i64,
    pub handle: String,
    pub display_name: String,
    pub credit_grant: i32,
    pub heroes: Vec<RosterHero>,
    /// This entry's exchanges, ascending by `(round, id)`. Empty for an entry
    /// that never swapped, which is every entry in a tournament that does not
    /// use the mechanic -- and then [`EntryRoster::holdings`] degenerates to
    /// "held everything from round 1", i.e. exactly the old behaviour.
    pub swaps: Vec<RosterSwap>,
}

impl EntryRoster {
    /// Derived from the slots rather than materialised, exactly like every
    /// other number here.
    ///
    /// This is the *current* roster's cost, which is what the leaderboard shows
    /// beside the manager -- not a per-round figure.
    pub fn spent(&self) -> i32 {
        self.heroes.iter().map(|h| h.cost).sum()
    }

    /// Which heroes this entry held, and over which rounds.
    ///
    /// Only the current roster is stored, so the history is reconstructed in
    /// two passes: undo every swap newest-first to recover the roster as it
    /// stood at the start, then replay them oldest-first, closing the outgoing
    /// hero's holding at that round and opening the incoming hero's there.
    ///
    /// This is why a swap must be recorded as an event rather than as a counter
    /// -- the log is not bookkeeping about an allowance, it is the only thing
    /// that can answer "who owned this hero in round 2".
    pub fn holdings(&self) -> Vec<Holding> {
        // Undo, newest first, to recover the opening roster.
        let mut opening: Vec<i64> = self.heroes.iter().map(|h| h.hero_id).collect();
        for swap in self.swaps.iter().rev() {
            match opening.iter().position(|&id| id == swap.hero_in_id) {
                Some(index) => opening[index] = swap.hero_out_id,
                // The incoming hero is not on the roster at this point in the
                // rewind, so a later swap already traded it away and has been
                // undone above. Its predecessor still returns: the log is the
                // record, and dropping it here would shorten the history.
                None => opening.push(swap.hero_out_id),
            }
        }

        let mut holdings: Vec<Holding> = opening
            .into_iter()
            .map(|hero_id| Holding {
                hero_id,
                from_round: 1,
                until_round: None,
            })
            .collect();

        // Replay, oldest first.
        for swap in &self.swaps {
            if let Some(open) = holdings
                .iter_mut()
                .find(|h| h.hero_id == swap.hero_out_id && h.until_round.is_none())
            {
                open.until_round = Some(swap.round);
            }
            holdings.push(Holding {
                hero_id: swap.hero_in_id,
                from_round: swap.round,
                until_round: None,
            });
        }

        holdings
    }
}

/// One leaderboard column.
///
/// The board carries its own column definitions because the backend does not
/// know which columns exist until it has read `scoring_coefficients` -- an admin
/// adds a metric with an INSERT, not a redeploy.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricColumn {
    pub metric: String,
    pub label: String,
    pub coefficient: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StandingsRow {
    pub rank: i32,
    pub entry_id: i64,
    pub manager_id: i64,
    pub handle: String,
    pub display_name: String,
    pub roster: Vec<String>,
    pub spent: i32,
    pub credit_grant: i32,
    pub total_points: f64,
    /// Points earned in the tournament's latest round -- the "LAST RD" figure.
    pub round_points: f64,
    /// Keyed by [`MetricColumn::metric`]. Unknown metrics never appear.
    pub breakdown: IndexMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StandingsBoard {
    pub tournament_id: i64,
    pub rule_set_name: String,
    pub current_round: i32,
    pub metrics: Vec<MetricColumn>,
    pub rows: Vec<StandingsRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickerGameSide {
    /// Free text, absent from the JSON when the result was recorded
    /// unattributed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_label: Option<String>,
    pub hero_name: String,
    pub health_remaining: i32,
    pub is_winner: bool,
    /// This hero's net score for this game. May be negative.
    pub points: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickerGame {
    pub game_number: i32,
    pub map_name: String,
    /// Winner first -- every game has one, so this is always winner then loser.
    pub sides: Vec<TickerGameSide>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickerEntry {
    pub match_id: i64,
    pub round: i32,
    #[serde(with = "java_instant")]
    pub played_at: DateTime<Utc>,
    pub external_link: String,
    /// Ordered by game number -- one entry per game played in the series.
    pub games: Vec<TickerGame>,
    pub banned_hero_names: Vec<String>,
    /// Heroes drafted for this series that never played a game. They appear in
    /// no [`TickerEntry::games`] row, so without naming them here their
    /// appearance points come from nowhere the reader can see.
    pub drafted_unplayed_hero_names: Vec<String>,
}

/// Every hero the tournament touched, priced once.
struct ScoredAppearance {
    round: i32,
    breakdown: IndexMap<String, f64>,
}

/// Folds recorded matches into a ranked leaderboard.
///
/// Each (hero, match) pair is priced exactly once regardless of how many
/// rosters hold that hero -- cheaper than the join's fan-out, and impossible to
/// get inconsistent with [`ticker`], which prices from the same contexts.
pub fn board(
    tournament_id: i64,
    matches: &[MatchResult],
    rules: &ScoringRules,
    rosters: &[EntryRoster],
) -> StandingsBoard {
    let current_round = matches.iter().map(|m| m.round).max().unwrap_or(0);

    // An `IndexMap` preserves first-encounter order.
    let mut appearances_by_hero: IndexMap<i64, Vec<ScoredAppearance>> = IndexMap::new();
    for match_result in matches {
        for context in match_result.hero_contexts() {
            appearances_by_hero
                .entry(context.hero_id)
                .or_default()
                .push(ScoredAppearance {
                    round: match_result.round,
                    breakdown: scoring_engine::breakdown(&context, rules),
                });
        }
    }

    let unranked: Vec<StandingsRow> = rosters
        .iter()
        .map(|entry| {
            // Dense: every scored metric gets a column value, even a zero one.
            // Sparse would leave the leaderboard's cells ragged against the
            // column list the board also carries.
            let mut totals: IndexMap<String, f64> = rules
                .scored_metrics()
                .iter()
                .map(|metric| (metric.clone(), 0.0))
                .collect();
            let mut round_points = 0.0;

            // Holdings, not the current roster: a hero scores for this manager
            // only in the rounds they actually held it. Without that gate a
            // between-round swap rewrites history in both directions -- the
            // departing hero's past points vanish and the arriving hero's
            // arrive backdated. An entry that never swapped has one holding per
            // hero, open from round 1, so this is the old fold exactly.
            for holding in entry.holdings() {
                let Some(appearances) = appearances_by_hero.get(&holding.hero_id) else {
                    continue;
                };
                for appearance in appearances {
                    if !holding.covers(appearance.round) {
                        continue;
                    }
                    for (metric, points) in &appearance.breakdown {
                        *totals.entry(metric.clone()).or_insert(0.0) += points;
                        if appearance.round == current_round {
                            round_points += points;
                        }
                    }
                }
            }

            let breakdown: IndexMap<String, f64> = totals
                .into_iter()
                .map(|(metric, points)| (metric, round2(points)))
                .collect();
            StandingsRow {
                // Replaced once the board is ordered.
                rank: 0,
                entry_id: entry.entry_id,
                manager_id: entry.manager_id,
                handle: entry.handle.clone(),
                display_name: entry.display_name.clone(),
                roster: entry.heroes.iter().map(|h| h.name.clone()).collect(),
                spent: entry.spent(),
                credit_grant: entry.credit_grant,
                total_points: round2(breakdown.values().sum()),
                round_points: round2(round_points),
                breakdown,
            }
        })
        .collect();

    StandingsBoard {
        tournament_id,
        rule_set_name: rules.name.clone(),
        current_round,
        metrics: rules
            .scored_metrics()
            .iter()
            .map(|metric| MetricColumn {
                metric: metric.clone(),
                label: match_metrics::label(metric),
                coefficient: rules
                    .coefficient_of(metric)
                    .try_into()
                    .expect("a numeric(10,4) coefficient always fits an f64"),
            })
            .collect(),
        rows: rank(unranked),
    }
}

/// Standard competition ranking (1, 2, 2, 4).
///
/// Ties are ordinary on a finished tournament -- two managers who drafted
/// overlapping rosters can genuinely land on the same total -- so a positional
/// `index + 1` would lie.
///
/// The comparison is exact `!=` on `f64` with **no epsilon**: every total has
/// already been through [`round2`], and an epsilon would manufacture ties
/// that don't actually exist. The sort is stable for the same reason the
/// ticker's is.
fn rank(mut rows: Vec<StandingsRow>) -> Vec<StandingsRow> {
    rows.sort_by(|a, b| {
        b.total_points
            .partial_cmp(&a.total_points)
            .expect("totals are finite: every one has been through round2")
            // Byte-order comparison on the handle as a deterministic tiebreak.
            .then_with(|| a.handle.cmp(&b.handle))
    });

    let mut current_rank = 0;
    let mut previous_points: Option<f64> = None;
    for (index, row) in rows.iter_mut().enumerate() {
        if previous_points != Some(row.total_points) {
            current_rank = index as i32 + 1;
            previous_points = Some(row.total_points);
        }
        row.rank = current_rank;
    }
    rows
}

/// The newest recorded matches, as the Standings ticker renders them.
///
/// `matches` arrives already sliced by the caller -- the polling key is
/// `sinceMatchId` rather than a timestamp, because parallel tables in a round
/// share a `played_at` while the match id is a monotonic `bigserial`.
pub fn ticker(matches: &[MatchResult], rules: &ScoringRules) -> Vec<TickerEntry> {
    matches
        .iter()
        .map(|match_result| ticker_entry(match_result, rules))
        .collect()
}

fn ticker_entry(match_result: &MatchResult, rules: &ScoringRules) -> TickerEntry {
    // Built once and partitioned by role, rather than re-filtered from a fresh
    // hero_contexts() call per role -- same list, two views onto it.
    let contexts = match_result.hero_contexts();

    // Keyed by (game, hero), not hero alone: the same hero can appear in two
    // different games of a series with two different scores.
    let mut contexts_by_game_and_hero = IndexMap::new();
    for context in &contexts {
        if let HeroRole::Played { game, .. } = context.role {
            // A later duplicate overwrites.
            contexts_by_game_and_hero.insert((game.game_id, context.hero_id), context);
        }
    }

    // A `Drafted` context is a match-level fact with no game of its own, but
    // the ticker only has game rows to show points in. Bank it against the
    // hero's first game so the rows still sum to what the match banked on the
    // board; in a Bo3 that makes game 1 worth one appearance more than games 2
    // and 3, which is the draft being priced once, not drift.
    let mut games_by_number: Vec<&_> = match_result.games.iter().collect();
    games_by_number.sort_by_key(|game| game.game_number);
    let mut first_game_id_by_hero: IndexMap<i64, i64> = IndexMap::new();
    for game in &games_by_number {
        for participant in &game.participants {
            // The earliest game wins.
            first_game_id_by_hero
                .entry(participant.hero_id)
                .or_insert(game.game_id);
        }
    }

    let mut draft_contexts_by_game_and_hero = IndexMap::new();
    for context in &contexts {
        if context.role == HeroRole::Drafted
            && let Some(&game_id) = first_game_id_by_hero.get(&context.hero_id)
        {
            draft_contexts_by_game_and_hero.insert((game_id, context.hero_id), context);
        }
    }

    let played_hero_ids: IndexSet<i64> = match_result
        .games
        .iter()
        .flat_map(|game| game.participants.iter().map(|p| p.hero_id))
        .collect();

    let games = match_result
        .games
        .iter()
        .map(|game| {
            // Stable sort: the winner floats up, the rest keep recorded order.
            let mut participants: Vec<&_> = game.participants.iter().collect();
            participants.sort_by_key(|p| std::cmp::Reverse(p.is_winner));

            TickerGame {
                game_number: game.game_number,
                map_name: game.map_name.clone(),
                sides: participants
                    .into_iter()
                    .map(|participant| {
                        let key = (game.game_id, participant.hero_id);
                        // Each context is scored (and so rounded) on its own,
                        // then the pair is summed and rounded again -- the
                        // double rounding is not a simplification to remove.
                        let points: f64 = contexts_by_game_and_hero
                            .get(&key)
                            .into_iter()
                            .chain(draft_contexts_by_game_and_hero.get(&key))
                            .map(|context| scoring_engine::score(context, rules))
                            .sum();
                        TickerGameSide {
                            player_label: match_result
                                .player_label_for_side(participant.side)
                                .map(str::to_owned),
                            hero_name: participant.hero_name.clone(),
                            health_remaining: participant.health_remaining,
                            is_winner: participant.is_winner,
                            points: round2(points),
                        }
                    })
                    .collect(),
            }
        })
        .collect();

    // `IndexSet` de-duplicates by hero id: first name wins, and the order is
    // the order the drafts were read in.
    let mut unplayed: IndexSet<i64> = IndexSet::new();
    let mut drafted_unplayed_hero_names = Vec::new();
    for participant in &match_result.participants {
        for hero in &participant.drafted_heroes {
            if !played_hero_ids.contains(&hero.hero_id) && unplayed.insert(hero.hero_id) {
                drafted_unplayed_hero_names.push(hero.hero_name.clone());
            }
        }
    }

    TickerEntry {
        match_id: match_result.match_id,
        round: match_result.round,
        played_at: match_result.played_at,
        external_link: match_result.external_link.clone(),
        games,
        banned_hero_names: match_result
            .bans
            .iter()
            .map(|ban| ban.hero_name.clone())
            .collect(),
        drafted_unplayed_hero_names,
    }
}

/// The fold, tested as data.
///
/// Extracting the arithmetic out of the transactional service is what makes
/// the ranking, the dense breakdown, the round attribution and the ticker's
/// banking assertable on their own, rather than only through a full
/// database-backed integration test.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_result::{
        BanResult, BanType, DraftedHeroResult, GameParticipantResult, GameResult,
        MatchParticipantResult,
    };
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
    /// unimplemented, so it must never reach a column.
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

    fn side(side: i32, label: &str, heroes: Vec<DraftedHeroResult>) -> MatchParticipantResult {
        MatchParticipantResult {
            side,
            player_label: Some(label.into()),
            drafted_heroes: heroes,
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

    /// A single game: Bigfoot (7) beats Beowulf (11) on 11 health, with Sun
    /// Wukong (8) struck by an opponent ban and Alice (9) drafted but never
    /// fielded.
    fn one_game_match() -> MatchResult {
        MatchResult {
            match_id: 6,
            tournament_id: 1,
            round: 2,
            played_at: "2026-06-06T11:00:00Z".parse().unwrap(),
            external_link: "https://example.com/match/6".into(),
            participants: vec![
                side(
                    0,
                    "Aurelie Blanc",
                    vec![hero(7, "Bigfoot"), hero(9, "Alice")],
                ),
                side(1, "Miles Ashworth", vec![hero(11, "Beowulf")]),
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
                side: None,
            }],
        }
    }

    /// A Bo2 in which Bigfoot plays both games -- the fixture the ticker's
    /// draft banking needs.
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

    fn roster(entry_id: i64, handle: &str, heroes: &[(i64, &str, i32)]) -> EntryRoster {
        EntryRoster {
            entry_id,
            manager_id: entry_id + 100,
            handle: handle.into(),
            display_name: format!("{handle} Display"),
            credit_grant: 10_000,
            heroes: heroes
                .iter()
                .enumerate()
                .map(|(slot_index, (hero_id, name, cost))| RosterHero {
                    slot_index: slot_index as i32,
                    hero_id: *hero_id,
                    name: (*name).into(),
                    cost: *cost,
                })
                .collect(),
            swaps: Vec::new(),
        }
    }

    /// The same roster, plus the exchanges that produced it. `heroes` is always
    /// the roster held *now*, so a test that swaps must state the end state
    /// here and let [`EntryRoster::holdings`] rebuild the rest.
    fn roster_with_swaps(
        entry_id: i64,
        handle: &str,
        heroes: &[(i64, &str, i32)],
        swaps: &[(i32, i64, i64)],
    ) -> EntryRoster {
        EntryRoster {
            swaps: swaps
                .iter()
                .map(|&(round, hero_out_id, hero_in_id)| RosterSwap {
                    round,
                    hero_out_id,
                    hero_in_id,
                })
                .collect(),
            ..roster(entry_id, handle, heroes)
        }
    }

    /// A row scoring `points`, for the ranking cases -- rank is the one thing
    /// [`rank`] derives from the board rather than from a match.
    fn scored_row(handle: &str, points: f64) -> StandingsRow {
        StandingsRow {
            rank: 0,
            entry_id: 1,
            manager_id: 1,
            handle: handle.into(),
            display_name: handle.into(),
            roster: vec![],
            spent: 0,
            credit_grant: 10_000,
            total_points: points,
            round_points: 0.0,
            breakdown: IndexMap::new(),
        }
    }

    fn ranked(pairs: &[(&str, f64)]) -> Vec<(String, i32)> {
        let rows: Vec<StandingsRow> = pairs
            .iter()
            .map(|(handle, points)| scored_row(handle, *points))
            .collect();
        rank(rows)
            .into_iter()
            .map(|row| (row.handle, row.rank))
            .collect()
    }

    // -- ranking ------------------------------------------------------------

    #[test]
    fn ranking_is_standard_competition_ranking() {
        // 1, 2, 2, 4 -- the tie consumes the position after it, so no manager
        // is ever ranked third here.
        assert_eq!(
            ranked(&[("a", 10.0), ("b", 8.0), ("c", 8.0), ("d", 5.0)]),
            vec![
                ("a".into(), 1),
                ("b".into(), 2),
                ("c".into(), 2),
                ("d".into(), 4),
            ]
        );
    }

    #[test]
    fn ties_break_on_handle_so_the_board_is_deterministic() {
        // Equal totals arriving in the wrong order still come out
        // alphabetical: without the secondary key, two requests could disagree
        // about who is listed first. Both share rank 2, and the tie consumes
        // position 3.
        assert_eq!(
            ranked(&[("zoe", 8.0), ("adam", 8.0), ("mia", 9.0), ("rex", 1.0)]),
            vec![
                ("mia".into(), 1),
                ("adam".into(), 2),
                ("zoe".into(), 2),
                ("rex".into(), 4),
            ]
        );
    }

    #[test]
    fn an_all_tied_board_ranks_everyone_first() {
        assert_eq!(
            ranked(&[("a", 0.0), ("b", 0.0), ("c", 0.0)]),
            vec![("a".into(), 1), ("b".into(), 1), ("c".into(), 1)]
        );
    }

    #[test]
    fn totals_are_compared_exactly_with_no_epsilon() {
        // Both have been through round2, so 0.01 apart is a real difference
        // and not a tie to be smoothed away.
        assert_eq!(
            ranked(&[("a", 8.01), ("b", 8.0)]),
            vec![("a".into(), 1), ("b".into(), 2)]
        );
    }

    #[test]
    fn an_empty_board_ranks_nothing() {
        assert!(rank(vec![]).is_empty());
    }

    // -- board --------------------------------------------------------------

    #[test]
    fn the_board_prices_a_roster_from_the_matches_its_heroes_appeared_in() {
        let matches = vec![one_game_match()];
        let rosters = vec![roster(1, "ArthurianLegend", &[(7, "Bigfoot", 2500)])];

        let board = board(1, &matches, &standard(), &rosters);
        let row = &board.rows[0];

        // WIN 10 + HEALTH_REMAINING 11 * 0.75 = 8.25 + APPEARANCE 1.
        assert_eq!(row.breakdown["WIN"], 10.0);
        assert_eq!(row.breakdown["HEALTH_REMAINING"], 8.25);
        assert_eq!(row.breakdown["APPEARANCE"], 1.0);
        assert_eq!(row.total_points, 19.25);
        assert_eq!(row.rank, 1);
        assert_eq!(row.spent, 2500);
        assert_eq!(row.roster, vec!["Bigfoot".to_string()]);
    }

    #[test]
    fn the_breakdown_is_dense_every_scored_metric_gets_a_cell() {
        // A hero that earned nothing on a metric still has the column, so the
        // leaderboard's cells line up with the column list the board carries.
        let matches = vec![one_game_match()];
        let rosters = vec![roster(1, "ArthurianLegend", &[(11, "Beowulf", 1000)])];

        let board = board(1, &matches, &standard(), &rosters);
        let row = &board.rows[0];

        assert_eq!(
            row.breakdown.keys().collect::<Vec<_>>(),
            vec!["WIN", "HEALTH_REMAINING", "APPEARANCE", "OPPONENT_BAN"]
        );
        assert_eq!(row.breakdown["WIN"], 0.0);
        assert_eq!(row.total_points, 1.0);
    }

    #[test]
    fn an_unimplemented_metric_is_never_a_column() {
        // CROWD_FAVOURITE is weighted in the seed and deliberately has no
        // extractor. It scores zero, and it must not reach the board at all.
        let board = board(
            1,
            &[one_game_match()],
            &standard(),
            &[roster(1, "ArthurianLegend", &[(7, "Bigfoot", 2500)])],
        );

        let columns: Vec<&str> = board.metrics.iter().map(|m| m.metric.as_str()).collect();
        assert_eq!(
            columns,
            vec!["WIN", "HEALTH_REMAINING", "APPEARANCE", "OPPONENT_BAN"]
        );
        assert!(!board.rows[0].breakdown.contains_key("CROWD_FAVOURITE"));
    }

    #[test]
    fn columns_keep_the_rule_sets_own_order_and_carry_their_coefficient() {
        let board = board(1, &[one_game_match()], &standard(), &[]);

        assert_eq!(board.metrics[0].metric, "WIN");
        assert_eq!(board.metrics[0].label, "Win");
        assert_eq!(board.metrics[0].coefficient, 10.0);
        assert_eq!(board.metrics[1].coefficient, 0.75);
        assert_eq!(board.rule_set_name, "Season 2026 Standard");
    }

    #[test]
    fn round_points_count_only_the_latest_round() {
        let mut round_one = one_game_match();
        round_one.match_id = 1;
        round_one.round = 1;
        let round_two = one_game_match(); // round 2

        let matches = vec![round_one, round_two];
        let rosters = vec![roster(1, "ArthurianLegend", &[(7, "Bigfoot", 2500)])];

        let board = board(1, &matches, &standard(), &rosters);
        let row = &board.rows[0];

        assert_eq!(board.current_round, 2);
        // Both matches score 19.25; only the round-2 one is the round figure.
        assert_eq!(row.total_points, 38.5);
        assert_eq!(row.round_points, 19.25);
    }

    #[test]
    fn a_tournament_with_no_matches_still_boards_its_entries() {
        // current_round is 0 rather than absent, and an entry that has drafted
        // is on the board with a zero total -- the screen exists before the
        // first result is recorded.
        let rosters = vec![roster(1, "ArthurianLegend", &[(7, "Bigfoot", 2500)])];
        let board = board(1, &[], &standard(), &rosters);

        assert_eq!(board.current_round, 0);
        assert_eq!(board.rows.len(), 1);
        assert_eq!(board.rows[0].total_points, 0.0);
        assert_eq!(board.rows[0].round_points, 0.0);
        assert_eq!(board.rows[0].rank, 1);
    }

    #[test]
    fn an_entry_with_no_picks_is_still_on_the_board() {
        // The query behind this is a left join for exactly this reason.
        let board = board(
            1,
            &[one_game_match()],
            &standard(),
            &[roster(1, "NewJoiner", &[])],
        );

        assert_eq!(board.rows.len(), 1);
        assert_eq!(board.rows[0].spent, 0);
        assert!(board.rows[0].roster.is_empty());
        assert_eq!(board.rows[0].total_points, 0.0);
    }

    #[test]
    fn a_hero_on_two_rosters_is_priced_once_and_scores_for_both() {
        let matches = vec![one_game_match()];
        let rosters = vec![
            roster(1, "ArthurianLegend", &[(7, "Bigfoot", 2500)]),
            roster(2, "MythicMind", &[(7, "Bigfoot", 2500)]),
        ];

        let board = board(1, &matches, &standard(), &rosters);

        assert_eq!(board.rows[0].total_points, 19.25);
        assert_eq!(board.rows[1].total_points, 19.25);
    }

    #[test]
    fn a_hero_that_played_twice_scores_each_game_but_appears_once() {
        // The whole reason `hero_contexts` splits per-game from per-series:
        // WIN and HEALTH_REMAINING double, APPEARANCE does not.
        let rosters = vec![roster(1, "ArthurianLegend", &[(7, "Bigfoot", 2500)])];
        let board = board(1, &[two_game_match()], &standard(), &rosters);
        let row = &board.rows[0];

        assert_eq!(row.breakdown["WIN"], 20.0);
        assert_eq!(row.breakdown["HEALTH_REMAINING"], 11.25); // (11 + 4) * 0.75
        assert_eq!(row.breakdown["APPEARANCE"], 1.0);
    }

    #[test]
    fn a_rule_set_that_scores_nothing_leaves_an_empty_breakdown() {
        let board = board(
            1,
            &[one_game_match()],
            &ScoringRules::none(),
            &[roster(1, "ArthurianLegend", &[(7, "Bigfoot", 2500)])],
        );

        assert!(board.metrics.is_empty());
        assert!(board.rows[0].breakdown.is_empty());
        assert_eq!(board.rows[0].total_points, 0.0);
    }

    // -- ticker -------------------------------------------------------------

    #[test]
    fn the_ticker_puts_the_winner_first() {
        let entries = ticker(&[one_game_match()], &standard());
        let sides = &entries[0].games[0].sides;

        assert_eq!(sides[0].hero_name, "Bigfoot");
        assert!(sides[0].is_winner);
        assert_eq!(sides[1].hero_name, "Beowulf");
        assert_eq!(sides[0].player_label.as_deref(), Some("Aurelie Blanc"));
    }

    #[test]
    fn a_ticker_side_carries_that_heros_points_for_that_game() {
        let entries = ticker(&[one_game_match()], &standard());
        let sides = &entries[0].games[0].sides;

        // Winner: WIN 10 + health 8.25 + the banked APPEARANCE 1.
        assert_eq!(sides[0].points, 19.25);
        // Loser: nothing but its own appearance.
        assert_eq!(sides[1].points, 1.0);
    }

    #[test]
    fn the_draft_is_banked_against_a_heros_first_game_not_every_game() {
        // Game 1 carries the appearance; game 2 does not. Summing the ticker's
        // rows therefore reproduces what the board banked, which is the
        // property this banking exists for.
        let entries = ticker(&[two_game_match()], &standard());
        let games = &entries[0].games;

        assert_eq!(games[0].sides[0].points, 19.25); // 10 + 8.25 + 1
        assert_eq!(games[1].sides[0].points, 13.0); // 10 + 3.00, no appearance

        let bigfoot_total: f64 = games
            .iter()
            .flat_map(|g| &g.sides)
            .filter(|s| s.hero_name == "Bigfoot")
            .map(|s| s.points)
            .sum();
        let board = board(
            1,
            &[two_game_match()],
            &standard(),
            &[roster(1, "ArthurianLegend", &[(7, "Bigfoot", 2500)])],
        );
        assert_eq!(bigfoot_total, board.rows[0].total_points);
    }

    #[test]
    fn heroes_drafted_and_never_fielded_are_named_separately() {
        // Alice scored an appearance and is in no game row, so without this the
        // points would come from nowhere the reader can see.
        let entries = ticker(&[one_game_match()], &standard());

        assert_eq!(entries[0].drafted_unplayed_hero_names, vec!["Alice"]);
        assert_eq!(entries[0].banned_hero_names, vec!["Sun Wukong"]);
    }

    #[test]
    fn a_hero_drafted_by_both_sides_is_named_once_among_the_unfielded() {
        let mut m = one_game_match();
        m.participants[1].drafted_heroes.push(hero(9, "Alice"));

        let entries = ticker(&[m], &standard());

        assert_eq!(entries[0].drafted_unplayed_hero_names, vec!["Alice"]);
    }

    #[test]
    fn the_ticker_carries_the_matchs_own_identity() {
        let entries = ticker(&[one_game_match()], &standard());

        assert_eq!(entries[0].match_id, 6);
        assert_eq!(entries[0].round, 2);
        assert_eq!(entries[0].external_link, "https://example.com/match/6");
        assert_eq!(entries[0].games[0].map_name, "Raptor Paddock");
    }

    #[test]
    fn an_unattributed_side_omits_its_label_rather_than_emitting_null() {
        let mut m = one_game_match();
        m.participants[1].player_label = None;

        let entries = ticker(&[m], &standard());
        let json = serde_json::to_string(&entries[0].games[0].sides[1]).unwrap();

        assert!(!json.contains("playerLabel"), "{json}");
        assert!(!json.contains("null"), "{json}");
    }

    #[test]
    fn the_board_serializes_under_the_frozen_wire_names() {
        let board = board(
            1,
            &[one_game_match()],
            &standard(),
            &[roster(1, "ArthurianLegend", &[(7, "Bigfoot", 2500)])],
        );
        let json = serde_json::to_value(&board).unwrap();

        assert_eq!(json["tournamentId"], 1);
        assert_eq!(json["ruleSetName"], "Season 2026 Standard");
        assert_eq!(json["currentRound"], 2);
        assert_eq!(json["rows"][0]["totalPoints"], 19.25);
        assert_eq!(json["rows"][0]["roundPoints"], 19.25);
        assert_eq!(json["rows"][0]["creditGrant"], 10_000);
        assert_eq!(json["rows"][0]["displayName"], "ArthurianLegend Display");
        assert_eq!(json["rows"][0]["breakdown"]["WIN"], 10.0);
    }

    #[test]
    fn a_ticker_entry_serializes_its_timestamp_the_way_java_did() {
        let entries = ticker(&[one_game_match()], &standard());
        let json = serde_json::to_value(&entries[0]).unwrap();

        assert_eq!(json["playedAt"], "2026-06-06T11:00:00Z");
        assert_eq!(json["games"][0]["sides"][0]["isWinner"], true);
        assert_eq!(json["draftedUnplayedHeroNames"][0], "Alice");
    }

    // -- holdings -----------------------------------------------------------

    /// Sorted, so a test states which heroes were held over which rounds
    /// without depending on the order the replay happens to produce.
    fn spans(entry: &EntryRoster) -> Vec<(i64, i32, Option<i32>)> {
        let mut spans: Vec<(i64, i32, Option<i32>)> = entry
            .holdings()
            .into_iter()
            .map(|h| (h.hero_id, h.from_round, h.until_round))
            .collect();
        spans.sort();
        spans
    }

    #[test]
    fn an_entry_that_never_swapped_holds_everything_from_round_one() {
        let entry = roster(
            1,
            "ArthurianLegend",
            &[(7, "Bigfoot", 2500), (9, "Alice", 1200)],
        );
        assert_eq!(spans(&entry), vec![(7, 1, None), (9, 1, None)]);
    }

    #[test]
    fn a_swap_closes_the_departing_hero_and_opens_the_arriving_one() {
        // Held 7 and 9 from the start; traded 7 away for 11 in round 3.
        let entry = roster_with_swaps(
            1,
            "ArthurianLegend",
            &[(11, "Beowulf", 2400), (9, "Alice", 1200)],
            &[(3, 7, 11)],
        );
        assert_eq!(
            spans(&entry),
            vec![(7, 1, Some(3)), (9, 1, None), (11, 3, None)]
        );
    }

    /// The case an interval list exists for: one hero, two disjoint spells, and
    /// a gap in the middle it must not score in.
    #[test]
    fn a_hero_traded_away_and_bought_back_yields_two_holdings() {
        let entry = roster_with_swaps(
            1,
            "ArthurianLegend",
            &[(7, "Bigfoot", 2500)],
            &[(2, 7, 11), (4, 11, 7)],
        );
        assert_eq!(
            spans(&entry),
            vec![(7, 1, Some(2)), (7, 4, None), (11, 2, Some(4))]
        );
    }

    #[test]
    fn holdings_cover_exactly_the_rounds_they_span() {
        let holding = Holding {
            hero_id: 7,
            from_round: 2,
            until_round: Some(4),
        };
        assert!(!holding.covers(1));
        assert!(holding.covers(2));
        assert!(holding.covers(3));
        // Exclusive: round 4 is the round the hero left in.
        assert!(!holding.covers(4));

        let still_held = Holding {
            hero_id: 7,
            from_round: 2,
            until_round: None,
        };
        assert!(still_held.covers(99));
    }

    // -- round-scoped scoring -----------------------------------------------

    /// `one_game_match` scores Bigfoot 19.25 under `standard()`. Copies of it
    /// in other rounds let a test say which round a manager was holding him.
    fn match_in_round(match_id: i64, round: i32) -> MatchResult {
        MatchResult {
            match_id,
            round,
            ..one_game_match()
        }
    }

    /// The rule the whole feature turns on: trading a hero away does not
    /// unwind what he already earned.
    #[test]
    fn points_stay_in_the_round_they_were_earned_in() {
        let matches = [
            match_in_round(1, 1),
            match_in_round(2, 2),
            match_in_round(3, 3),
        ];

        // Held Bigfoot for rounds 1 and 2, then traded him for Beowulf in
        // round 3. Beowulf loses his game, so he brings a different number.
        let swapper = roster_with_swaps(1, "Swapper", &[(11, "Beowulf", 2400)], &[(3, 7, 11)]);
        // The same three matches, held throughout, for comparison.
        let stayer = roster(2, "Stayer", &[(7, "Bigfoot", 2500)]);

        let board = board(1, &matches, &standard(), &[swapper, stayer]);
        let by_handle: IndexMap<&str, &StandingsRow> = board
            .rows
            .iter()
            .map(|row| (row.handle.as_str(), row))
            .collect();

        // Bigfoot wins his game in all three rounds, at 10.0 a win. Counting
        // wins rather than totals keeps the assertion on the one hero whose
        // ownership actually moved.
        assert_eq!(
            by_handle["Stayer"].breakdown["WIN"], 30.0,
            "held the winner for all three rounds"
        );
        assert_eq!(
            by_handle["Swapper"].breakdown["WIN"], 20.0,
            "rounds 1 and 2 survive the trade; round 3 belongs to whoever holds him then"
        );
    }

    /// The other direction: buying a hero does not backdate what he scored for
    /// somebody else.
    #[test]
    fn an_arriving_hero_brings_no_history_with_him() {
        let matches = [match_in_round(1, 1), match_in_round(2, 2)];

        // Picked Bigfoot up only in round 2, so only round 2 counts. The hero
        // traded away (99) appears in no match, so nothing but Bigfoot moves
        // the total.
        let latecomer = roster_with_swaps(1, "Latecomer", &[(7, "Bigfoot", 2500)], &[(2, 99, 7)]);
        let board = board(1, &matches, &standard(), &[latecomer]);

        assert_eq!(
            board.rows[0].total_points, 19.25,
            "one match's worth, not two -- round 1 was somebody else's"
        );
    }

    /// A hero out of the roster during the gap scores nothing for that round,
    /// even though the manager owns him at both ends of it.
    #[test]
    fn the_gap_between_two_holdings_scores_nothing() {
        let matches = [
            match_in_round(1, 1),
            match_in_round(2, 2),
            match_in_round(3, 3),
        ];

        let round_trip = roster_with_swaps(
            1,
            "RoundTrip",
            &[(7, "Bigfoot", 2500)],
            &[(2, 7, 9), (3, 9, 7)],
        );
        let board = board(1, &matches, &standard(), &[round_trip]);

        // Rounds 1 and 3 only: Alice (9) is drafted-but-unplayed in this
        // fixture and scores an appearance, so compare against Bigfoot's own
        // contribution rather than the flat total.
        assert_eq!(board.rows[0].breakdown["WIN"], 20.0, "two wins, not three");
    }

    /// An entry that never swapped must fold exactly as it did before the
    /// mechanic existed -- which is every entry in a tournament not using it.
    #[test]
    fn an_unswapped_roster_scores_what_it_always_did() {
        let matches = [match_in_round(1, 1), match_in_round(2, 2)];
        let plain = roster(1, "ArthurianLegend", &[(7, "Bigfoot", 2500)]);

        assert_eq!(
            board(1, &matches, &standard(), &[plain]).rows[0].total_points,
            38.50
        );
    }
}
