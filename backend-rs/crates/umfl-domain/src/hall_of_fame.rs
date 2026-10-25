//! The Hall of Fame: every completed tournament's final board, in; each
//! tournament's podium plus an all-time titles table, out.
//!
//! **Nothing here stores a winner.** A podium is the top of the board the
//! leaderboard already folds, read at request time, so a match corrected after
//! a tournament finished re-crowns its champion on the next read -- the same
//! reasoning as AGENTS.md's "Nothing writes points". The titles table is a
//! count over those podiums, and so is derived the same way.

use crate::standings::{StandingsBoard, StandingsRow};
use crate::tournament::TournamentFormat;
use chrono::NaiveDate;
use indexmap::IndexMap;
use serde::Serialize;

/// The deepest rank a podium reaches. Ties share a rank, so a podium can hold
/// more than this many rows -- two managers tied for third are both on it.
pub const PODIUM_DEPTH: i32 = 3;

/// What the Hall of Fame needs to know about a tournament besides its board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedTournament {
    pub tournament_id: i64,
    pub name: String,
    pub format: TournamentFormat,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
}

/// One completed tournament and the managers who finished on its podium.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HallOfFameEntry {
    pub tournament_id: i64,
    pub name: String,
    pub format: TournamentFormat,
    pub start_date: NaiveDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<NaiveDate>,
    pub rule_set_name: String,
    /// Every entry on the final board, not only the podium.
    pub entrants: usize,
    /// The board's own rows with a rank of [`PODIUM_DEPTH`] or better, in the
    /// board's order. Empty for a tournament nobody entered.
    pub podium: Vec<StandingsRow>,
}

/// One manager's record across every completed tournament.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleRow {
    pub rank: i32,
    pub manager_id: i64,
    pub handle: String,
    pub display_name: String,
    /// Tournaments finished at rank 1, a shared first place included.
    pub titles: u32,
    /// Tournaments finished on the podium, titles included.
    pub podiums: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HallOfFame {
    pub titles: Vec<TitleRow>,
    /// Newest first.
    pub tournaments: Vec<HallOfFameEntry>,
}

/// Folds the completed tournaments' boards into the Hall of Fame.
///
/// Tournaments are ordered newest first by end date, falling back to the start
/// date for one that was completed without an end date ever being entered; the
/// id breaks a tie so two requests never disagree about the order.
pub fn build(completed: &[(CompletedTournament, StandingsBoard)]) -> HallOfFame {
    let mut tournaments: Vec<HallOfFameEntry> = completed
        .iter()
        .map(|(tournament, board)| HallOfFameEntry {
            tournament_id: tournament.tournament_id,
            name: tournament.name.clone(),
            format: tournament.format,
            start_date: tournament.start_date,
            end_date: tournament.end_date,
            rule_set_name: board.rule_set_name.clone(),
            entrants: board.rows.len(),
            podium: board
                .rows
                .iter()
                .filter(|row| row.rank <= PODIUM_DEPTH)
                .cloned()
                .collect(),
        })
        .collect();
    tournaments.sort_by(|a, b| {
        finished_on(b)
            .cmp(&finished_on(a))
            .then_with(|| b.tournament_id.cmp(&a.tournament_id))
    });

    HallOfFame {
        titles: titles(&tournaments),
        tournaments,
    }
}

fn finished_on(entry: &HallOfFameEntry) -> NaiveDate {
    entry.end_date.unwrap_or(entry.start_date)
}

/// Counts each manager's titles and podiums, ranked by titles then podiums.
///
/// Standard competition ranking (1, 2, 2, 4) on the *pair*: two managers share
/// a rank only when both counts match. The handle orders rows within a tie and
/// never splits one. The name shown is the one from the newest tournament,
/// since `tournaments` arrives newest first.
fn titles(tournaments: &[HallOfFameEntry]) -> Vec<TitleRow> {
    let mut by_manager: IndexMap<i64, TitleRow> = IndexMap::new();
    for row in tournaments.iter().flat_map(|entry| &entry.podium) {
        let record = by_manager
            .entry(row.manager_id)
            .or_insert_with(|| TitleRow {
                rank: 0,
                manager_id: row.manager_id,
                handle: row.handle.clone(),
                display_name: row.display_name.clone(),
                titles: 0,
                podiums: 0,
            });
        record.podiums += 1;
        if row.rank == 1 {
            record.titles += 1;
        }
    }

    let mut rows: Vec<TitleRow> = by_manager.into_values().collect();
    rows.sort_by(|a, b| {
        b.titles
            .cmp(&a.titles)
            .then_with(|| b.podiums.cmp(&a.podiums))
            .then_with(|| a.handle.cmp(&b.handle))
    });

    let mut current_rank = 0;
    let mut previous: Option<(u32, u32)> = None;
    for (index, row) in rows.iter_mut().enumerate() {
        let record = (row.titles, row.podiums);
        if previous != Some(record) {
            current_rank = index as i32 + 1;
            previous = Some(record);
        }
        row.rank = current_rank;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(rank: i32, manager_id: i64, handle: &str, total_points: f64) -> StandingsRow {
        StandingsRow {
            rank,
            entry_id: manager_id * 100,
            manager_id,
            handle: handle.to_string(),
            display_name: format!("{handle} Display"),
            roster: vec!["Medusa".to_string()],
            spent: 1000,
            credit_grant: 10000,
            total_points,
            round_points: 0.0,
            breakdown: IndexMap::new(),
        }
    }

    fn tournament(
        id: i64,
        start: (i32, u32, u32),
        end: Option<(i32, u32, u32)>,
        rows: Vec<StandingsRow>,
    ) -> (CompletedTournament, StandingsBoard) {
        let date = |(y, m, d): (i32, u32, u32)| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        (
            CompletedTournament {
                tournament_id: id,
                name: format!("Tournament {id}"),
                format: TournamentFormat::Banquest,
                start_date: date(start),
                end_date: end.map(date),
            },
            StandingsBoard {
                tournament_id: id,
                rule_set_name: "Standard".to_string(),
                current_round: 3,
                metrics: vec![],
                rows,
            },
        )
    }

    fn handles(rows: &[StandingsRow]) -> Vec<&str> {
        rows.iter().map(|r| r.handle.as_str()).collect()
    }

    #[test]
    fn the_podium_is_the_top_three_ranks_of_the_board() {
        let hall = build(&[tournament(
            1,
            (2026, 6, 5),
            Some((2026, 6, 7)),
            vec![
                row(1, 1, "Alpha", 90.0),
                row(2, 2, "Bravo", 80.0),
                row(3, 3, "Charlie", 70.0),
                row(4, 4, "Delta", 60.0),
            ],
        )]);

        let entry = &hall.tournaments[0];
        assert_eq!(handles(&entry.podium), ["Alpha", "Bravo", "Charlie"]);
        assert_eq!(entry.entrants, 4);
        assert_eq!(entry.rule_set_name, "Standard");
    }

    #[test]
    fn a_tie_for_third_puts_both_on_the_podium() {
        let hall = build(&[tournament(
            1,
            (2026, 6, 5),
            None,
            vec![
                row(1, 1, "Alpha", 90.0),
                row(2, 2, "Bravo", 80.0),
                row(3, 3, "Charlie", 70.0),
                row(3, 4, "Delta", 70.0),
                row(5, 5, "Echo", 60.0),
            ],
        )]);

        assert_eq!(
            handles(&hall.tournaments[0].podium),
            ["Alpha", "Bravo", "Charlie", "Delta"]
        );
    }

    #[test]
    fn a_shared_first_place_is_a_title_for_both() {
        let hall = build(&[tournament(
            1,
            (2026, 6, 5),
            None,
            vec![
                row(1, 1, "Alpha", 90.0),
                row(1, 2, "Bravo", 90.0),
                row(3, 3, "Charlie", 70.0),
            ],
        )]);

        let alpha = hall.titles.iter().find(|t| t.handle == "Alpha").unwrap();
        let bravo = hall.titles.iter().find(|t| t.handle == "Bravo").unwrap();
        assert_eq!((alpha.titles, alpha.rank), (1, 1));
        assert_eq!((bravo.titles, bravo.rank), (1, 1));
    }

    #[test]
    fn titles_count_across_tournaments_and_rank_by_titles_then_podiums() {
        let hall = build(&[
            tournament(
                1,
                (2026, 1, 1),
                None,
                vec![
                    row(1, 1, "Alpha", 90.0),
                    row(2, 2, "Bravo", 80.0),
                    row(3, 3, "Charlie", 70.0),
                ],
            ),
            tournament(
                2,
                (2026, 2, 1),
                None,
                vec![
                    row(1, 1, "Alpha", 90.0),
                    row(2, 3, "Charlie", 80.0),
                    row(3, 4, "Delta", 70.0),
                ],
            ),
            tournament(
                3,
                (2026, 3, 1),
                None,
                vec![
                    row(1, 2, "Bravo", 90.0),
                    row(2, 4, "Delta", 80.0),
                    row(3, 5, "Echo", 70.0),
                ],
            ),
        ]);

        let table: Vec<(i32, &str, u32, u32)> = hall
            .titles
            .iter()
            .map(|t| (t.rank, t.handle.as_str(), t.titles, t.podiums))
            .collect();
        assert_eq!(
            table,
            [
                (1, "Alpha", 2, 2),
                (2, "Bravo", 1, 2),
                (3, "Charlie", 0, 2),
                (3, "Delta", 0, 2),
                (5, "Echo", 0, 1),
            ]
        );
    }

    #[test]
    fn tournaments_are_listed_newest_first_by_end_then_start_date() {
        let hall = build(&[
            tournament(1, (2026, 1, 1), Some((2026, 1, 3)), vec![]),
            // No end date: ordered by its start date instead.
            tournament(2, (2026, 5, 1), None, vec![]),
            tournament(3, (2026, 3, 1), Some((2026, 3, 2)), vec![]),
        ]);

        let ids: Vec<i64> = hall.tournaments.iter().map(|t| t.tournament_id).collect();
        assert_eq!(ids, [2, 3, 1]);
    }

    #[test]
    fn a_tournament_nobody_entered_is_listed_with_an_empty_podium() {
        let hall = build(&[tournament(1, (2026, 1, 1), None, vec![])]);

        assert_eq!(hall.tournaments.len(), 1);
        assert!(hall.tournaments[0].podium.is_empty());
        assert_eq!(hall.tournaments[0].entrants, 0);
        assert!(hall.titles.is_empty());
    }

    #[test]
    fn an_absent_end_date_is_absent_on_the_wire() {
        let hall = build(&[tournament(1, (2026, 1, 1), None, vec![])]);
        let json = serde_json::to_value(&hall).unwrap();

        let entry = &json["tournaments"][0];
        assert!(entry.get("endDate").is_none());
        assert_eq!(entry["startDate"], "2026-01-01");
        assert_eq!(entry["format"], "BANQUEST");
    }
}
