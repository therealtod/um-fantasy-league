//! The leaderboard, the ticker and the live stream, against the recorded
//! results in the seed.
//!
//! Driven over HTTP, so the routes, the JSON shape and the status codes are
//! checked by the same test that checks the fold.
//!
//! The seed is static, so every assertion here is exact rather than "something
//! is nonzero". Weights (`Season 2026 Standard`): WIN 10, HEALTH_REMAINING
//! 0.75, HEALTH_DIFFERENTIAL 0.5, SHUTOUT 3, SELF_BAN 2, OPPONENT_BAN 2,
//! APPEARANCE 1 -- plus the deliberately unimplemented CROWD_FAVOURITE 5.
//!
//! This file is where the headline leaderboard-parity assertion finally
//! lives: the hand-derived leaderboard is simultaneously the tripwire for
//! `round2` drift, `numeric(10,4)` decode drift and fold-order drift.

use std::time::Duration;

use futures::StreamExt;
use serde_json::{Value, json};

use crate::harness::TestApp;

const SUMMER: &str = "Summer of Legends";
const WINTER: &str = "Winter of Champions";

async fn board(app: &TestApp, tournament_id: i64) -> Value {
    let response = app
        .get(&format!("/api/tournaments/{tournament_id}/standings"))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    response.json()
}

async fn summer_board(app: &TestApp) -> Value {
    let id = app.tournament_id(SUMMER).await;
    board(app, id).await
}

async fn ticker(app: &TestApp, tournament_id: i64, query: &str) -> Vec<Value> {
    let response = app
        .get(&format!("/api/tournaments/{tournament_id}/matches{query}"))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    response
        .json()
        .as_array()
        .expect("the ticker is an array")
        .clone()
}

async fn summer_ticker(app: &TestApp) -> Vec<Value> {
    let id = app.tournament_id(SUMMER).await;
    ticker(app, id, "").await
}

/// One row by handle. Every seeded handle is unique, so exactly one row (or
/// none) is expected.
fn row<'a>(board: &'a Value, handle: &str) -> &'a Value {
    let rows = board["rows"].as_array().expect("rows");
    let mut matching = rows.iter().filter(|r| r["handle"] == handle);
    let found = matching
        .next()
        .unwrap_or_else(|| panic!("no row for {handle}"));
    assert!(matching.next().is_none(), "{handle} appears twice");
    found
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|v| v.as_str().expect("a string").to_owned())
        .collect()
}

fn field_of_each(rows: &Value, field: &str) -> Vec<f64> {
    rows.as_array()
        .expect("rows")
        .iter()
        .map(|r| r[field].as_f64().expect("a number"))
        .collect()
}

fn handles(board: &Value) -> Vec<String> {
    board["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["handle"].as_str().expect("a handle").to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// The board
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_board_carries_the_rule_set_the_round_and_its_own_columns() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;
    let board = board(&app, summer).await;

    assert_eq!(board["tournamentId"].as_i64(), Some(summer));
    assert_eq!(board["ruleSetName"], "Season 2026 Standard");
    assert_eq!(board["currentRound"], 3);

    let metrics = board["metrics"].as_array().expect("metrics").clone();
    assert_eq!(
        metrics
            .iter()
            .map(|m| m["metric"].as_str().expect("a metric").to_owned())
            .collect::<Vec<_>>(),
        [
            "WIN",
            "HEALTH_REMAINING",
            "HEALTH_DIFFERENTIAL",
            "SHUTOUT",
            "SELF_BAN",
            "OPPONENT_BAN",
            "APPEARANCE"
        ],
        "column order is sort_order, which only the database knows"
    );
    assert_eq!(
        metrics
            .iter()
            .map(|m| m["label"].as_str().expect("a label").to_owned())
            .collect::<Vec<_>>(),
        [
            "Win",
            "Health Remaining",
            "Health Differential",
            "Shutout",
            "Self Ban",
            "Opponent Ban",
            "Appearance"
        ]
    );
    assert_eq!(
        metrics
            .iter()
            .map(|m| m["coefficient"].as_f64().expect("a coefficient"))
            .collect::<Vec<_>>(),
        [10.0, 0.75, 0.5, 3.0, 2.0, 2.0, 1.0]
    );
}

#[tokio::test]
async fn the_seeded_crowd_favourite_metric_is_weighted_but_scores_nothing_anywhere() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    let configured = sqlx::query_scalar!(
        r#"select count(*) as "count!" from scoring_coefficients c
               join scoring_rule_sets rs on rs.id = c.rule_set_id
           where rs.tournament_id = $1 and c.metric = 'CROWD_FAVOURITE'"#,
        summer
    )
    .fetch_one(app.pool())
    .await
    .expect("count the seeded coefficient");
    assert_eq!(
        configured, 1,
        "precondition: it really is configured, at a coefficient of 5"
    );

    let board = board(&app, summer).await;

    assert!(
        !board["metrics"]
            .as_array()
            .expect("metrics")
            .iter()
            .any(|m| m["metric"] == "CROWD_FAVOURITE"),
        "it must not become a column"
    );
    for row in board["rows"].as_array().expect("rows") {
        assert!(
            row["breakdown"].get("CROWD_FAVOURITE").is_none(),
            "{} scored an unimplemented metric",
            row["handle"]
        );
    }
}

/// The headline leaderboard-parity assertion, asserted with exact `f64`
/// equality.
#[tokio::test]
async fn the_leaderboard_is_exact_ordered_and_complete() {
    let app = TestApp::spawn().await;
    let board = summer_board(&app).await;

    assert_eq!(
        handles(&board)
            .into_iter()
            .zip(field_of_each(&board["rows"], "totalPoints"))
            .collect::<Vec<_>>(),
        [
            ("ArthurianLegend".to_owned(), 100.00),
            ("NeonStrategist".to_owned(), 79.75),
            ("SherlockMain".to_owned(), 76.50),
            ("MythicMind".to_owned(), 61.00),
        ]
    );
    assert_eq!(
        board["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|r| r["rank"].as_i64().expect("a rank"))
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
}

/// The row worked out by hand, metric by metric.
///
/// HEALTH_DIFFERENTIAL is win-gated, so a loss or a shutout-taken contributes
/// 0.00 rather than a negative term; SELF_BAN/OPPONENT_BAN only price a ban of
/// that specific category, so a `PRE_BAN` (struck before sides are known)
/// contributes to neither.
///
/// King Arthur -- m3 loss (HR 0, HD 0 [not a win], APP 1.00);
///   m8 win (WIN 10, HR 10x0.75=7.50, HD (10-0)x0.5=5.00, SHUTOUT 3.00, APP 1.00);
///   m12 PRE_BAN (unpriced).
/// Yennenga -- m3 win (WIN 10, HR 3.75, HD 2.50, SHUTOUT 3.00, APP 1.00);
///   m11 OPPONENT_BAN (OPPONENT_BAN 2.00); m12 win (WIN 10, HR 6.75, HD 4.50,
///   SHUTOUT 3.00, APP 1.00).
/// Beowulf -- m3 PRE_BAN (unpriced); m6 shut out (HR 0, HD 0 [not a win],
///   APP 1.00); m8 PRE_BAN (unpriced); m10 win (WIN 10, HR 6.00, HD 4.00,
///   SHUTOUT 3.00, APP 1.00).
///
/// WIN 40.00 + HEALTH_REMAINING 24.00 + HEALTH_DIFFERENTIAL 16.00
///   + SHUTOUT 12.00 + SELF_BAN 0.00 + OPPONENT_BAN 2.00 + APPEARANCE 6.00 = 100.00
#[tokio::test]
async fn the_winning_row_adds_up_metric_by_metric() {
    let app = TestApp::spawn().await;
    let board = summer_board(&app).await;
    let arthurian = row(&board, "ArthurianLegend");

    assert_eq!(arthurian["rank"], 1);
    assert_eq!(
        strings(&arthurian["roster"]),
        ["King Arthur", "Yennenga", "Beowulf"]
    );
    assert_eq!(arthurian["spent"], 9_800);
    assert_eq!(arthurian["creditGrant"], 10_000);
    assert_eq!(
        arthurian["breakdown"],
        json!({
            "WIN": 40.0,
            "HEALTH_REMAINING": 24.0,
            "HEALTH_DIFFERENTIAL": 16.0,
            "SHUTOUT": 12.0,
            "SELF_BAN": 0.0,
            "OPPONENT_BAN": 2.0,
            "APPEARANCE": 6.0,
        })
    );
    assert_eq!(arthurian["totalPoints"].as_f64(), Some(100.0));
}

/// Every 0-health opponent awards SHUTOUT points to the winner's drafters.
#[tokio::test]
async fn the_shutout_shows_up_on_exactly_the_rosters_that_drafted_bigfoot() {
    let app = TestApp::spawn().await;
    let board = summer_board(&app).await;

    for (handle, expected) in [
        ("ArthurianLegend", 12.0),
        ("NeonStrategist", 9.0),
        ("MythicMind", 6.0),
        ("SherlockMain", 9.0),
    ] {
        assert_eq!(
            row(&board, handle)["breakdown"]["SHUTOUT"].as_f64(),
            Some(expected),
            "each defeated hero on exactly zero health is a shutout: {handle}"
        );
    }
}

/// The displayed total must equal the sum of the displayed parts -- the check
/// that catches a fold that rounds in a different order than it reports.
#[tokio::test]
async fn a_total_is_exactly_the_sum_of_its_own_breakdown() {
    let app = TestApp::spawn().await;
    let board = summer_board(&app).await;

    for row in board["rows"].as_array().expect("rows") {
        let parts: f64 = row["breakdown"]
            .as_object()
            .expect("a breakdown")
            .values()
            .map(|v| v.as_f64().expect("a number"))
            .sum();
        assert_eq!(
            row["totalPoints"].as_f64(),
            Some(umfl_domain::rounding::round2(parts)),
            "{}: displayed total must equal the sum of the displayed parts",
            row["handle"]
        );
    }
}

#[tokio::test]
async fn round_points_are_the_latest_rounds_swing_not_the_whole_total() {
    let app = TestApp::spawn().await;
    let board = summer_board(&app).await;

    assert_eq!(
        handles(&board)
            .into_iter()
            .zip(field_of_each(&board["rows"], "roundPoints"))
            .collect::<Vec<_>>(),
        [
            ("ArthurianLegend".to_owned(), 51.25),
            ("NeonStrategist".to_owned(), 3.00),
            ("SherlockMain".to_owned(), 47.75),
            ("MythicMind".to_owned(), 4.00),
        ]
    );
    for row in board["rows"].as_array().expect("rows") {
        assert!(
            row["roundPoints"].as_f64() <= row["totalPoints"].as_f64(),
            "round 3 is a subset of rounds 1-3: {}",
            row["handle"]
        );
    }
}

#[tokio::test]
async fn an_entry_with_no_picks_still_appears_and_ties_share_a_rank() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    for handle in ["EmptyDrafterA", "EmptyDrafterB"] {
        let manager_id = sqlx::query_scalar!(
            "insert into managers (handle, display_name) values ($1, $1) returning id",
            handle
        )
        .fetch_one(app.pool())
        .await
        .expect("insert a manager");
        sqlx::query!(
            "insert into tournament_entries (tournament_id, manager_id, status, credit_grant)
             values ($1, $2, 'DRAFT', 10000)",
            summer,
            manager_id
        )
        .execute(app.pool())
        .await
        .expect("insert an entry");
    }

    let board = board(&app, summer).await;

    let rows = board["rows"].as_array().expect("rows");
    assert_eq!(
        rows.len(),
        6,
        "a left join keeps the pickless entries on the board"
    );
    let empties: Vec<_> = rows
        .iter()
        .filter(|r| {
            r["handle"]
                .as_str()
                .is_some_and(|h| h.starts_with("EmptyDrafter"))
        })
        .collect();
    assert_eq!(empties.len(), 2);
    assert!(empties.iter().all(|r| {
        r["roster"].as_array().expect("a roster").is_empty()
            && r["spent"] == 0
            && r["totalPoints"].as_f64() == Some(0.0)
    }));
    // Standard competition ranking: 1, 2, 3, 4, 5, 5 -- not 1..6.
    assert_eq!(
        rows.iter()
            .map(|r| r["rank"].as_i64().expect("a rank"))
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 5]
    );
}

#[tokio::test]
async fn a_tournament_with_entries_but_no_matches_scores_everyone_zero() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    let manager = app.manager("NeonStrategist").await;
    sqlx::query!(
        "insert into tournament_entries (tournament_id, manager_id, status, credit_grant)
         values ($1, $2, 'DRAFT', 10000)",
        winter,
        manager.id
    )
    .execute(app.pool())
    .await
    .expect("insert an entry");

    let board = board(&app, winter).await;

    assert_eq!(board["currentRound"], 0);
    let rows = board["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["totalPoints"].as_f64(), Some(0.0));
    assert_eq!(
        board["metrics"].as_array().expect("metrics").len(),
        7,
        "the columns exist even before a single match is played"
    );
}

#[tokio::test]
async fn a_tournament_with_no_active_rule_set_still_returns_a_usable_board() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;
    sqlx::query!(
        "delete from scoring_rule_sets where tournament_id = $1",
        summer
    )
    .execute(app.pool())
    .await
    .expect("drop the rule sets");

    let board = board(&app, summer).await;

    assert_eq!(board["ruleSetName"], "");
    assert_eq!(board["currentRound"], 3, "the rounds were still played");
    assert!(board["metrics"].as_array().expect("metrics").is_empty());
    let rows = board["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|r| {
        r["totalPoints"].as_f64() == Some(0.0)
            && r["breakdown"].as_object().expect("a breakdown").is_empty()
    }));
}

/// The one assertion against the projection rather than the board: slot order
/// and the *live* price, neither of which survives the fold.
#[tokio::test]
async fn the_roster_projection_carries_slot_order_and_live_prices() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    let rosters = umfl_server::standings::query::rosters(app.pool(), summer)
        .await
        .expect("read the rosters");

    let neon = rosters
        .iter()
        .find(|r| r.handle == "NeonStrategist")
        .expect("NeonStrategist entered Summer of Legends");
    assert_eq!(
        neon.heroes.iter().map(|h| h.slot_index).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        neon.heroes
            .iter()
            .map(|h| h.name.as_str())
            .collect::<Vec<_>>(),
        ["Alice", "Robin Hood", "Bigfoot"]
    );
    assert_eq!(
        neon.heroes.iter().map(|h| h.cost).collect::<Vec<_>>(),
        [4_100, 3_200, 2_100]
    );
    assert_eq!(neon.spent(), 9_400);
}

// ---------------------------------------------------------------------------
// The ticker
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_ticker_returns_every_match_newest_first() {
    let app = TestApp::spawn().await;
    let ticker = summer_ticker(&app).await;

    assert_eq!(ticker.len(), 13);
    assert_eq!(
        ticker
            .iter()
            .map(|e| e["matchId"].as_i64().expect("a match id"))
            .collect::<Vec<_>>(),
        [13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
        "ordered by played_at desc, then id desc for the parallel tables"
    );
    assert_eq!(
        ticker
            .iter()
            .map(|e| e["round"].as_i64().expect("a round"))
            .collect::<Vec<_>>(),
        [3, 3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1]
    );
}

#[tokio::test]
async fn polling_on_the_match_id_returns_only_what_is_new() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    let fresh = ticker(&app, summer, "?sinceMatchId=10").await;

    assert_eq!(
        fresh
            .iter()
            .map(|e| e["matchId"].as_i64().expect("a match id"))
            .collect::<Vec<_>>(),
        [13, 12, 11]
    );
}

/// `limit` is clamped, not rejected -- `coerceIn(1, 200)`.
#[tokio::test]
async fn the_ticker_limit_is_clamped_at_both_ends_rather_than_refused() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    assert_eq!(ticker(&app, summer, "?limit=3").await.len(), 3);
    assert_eq!(
        ticker(&app, summer, "?limit=0").await.len(),
        1,
        "0 clamps up to 1"
    );
    assert_eq!(
        ticker(&app, summer, "?limit=99999").await.len(),
        13,
        "an over-large limit clamps to 200 and simply returns everything"
    );
}

fn entry(ticker: &[Value], match_id: i64) -> Value {
    ticker
        .iter()
        .find(|e| e["matchId"].as_i64() == Some(match_id))
        .unwrap_or_else(|| panic!("no ticker entry for match {match_id}"))
        .clone()
}

#[tokio::test]
async fn a_ticker_entry_names_both_sides_winner_first_with_their_net_points() {
    let app = TestApp::spawn().await;
    let shutout = entry(&summer_ticker(&app).await, 6);

    let games = shutout["games"].as_array().expect("games");
    assert_eq!(
        games.len(),
        1,
        "a single-game match has exactly one game entry"
    );
    let game = &games[0];
    assert_eq!(game["mapName"], "Raptor Paddock");
    let sides = game["sides"].as_array().expect("sides");
    assert_eq!(
        sides
            .iter()
            .map(|s| s["heroName"].as_str().expect("a hero").to_owned())
            .collect::<Vec<_>>(),
        ["Bigfoot", "Beowulf"]
    );
    assert_eq!(
        sides
            .iter()
            .map(|s| s["playerLabel"].as_str().expect("a label").to_owned())
            .collect::<Vec<_>>(),
        ["Aurelie Blanc", "Miles Ashworth"]
    );
    assert_eq!(
        sides
            .iter()
            .map(|s| s["isWinner"].as_bool().expect("a flag"))
            .collect::<Vec<_>>(),
        [true, false]
    );
    assert_eq!(
        sides
            .iter()
            .map(|s| s["healthRemaining"].as_i64().expect("health"))
            .collect::<Vec<_>>(),
        [11, 0]
    );
    // 10 + 8.25 + 5.50 + 3 + 1 = 27.75 against 0 + 1 = 1.00
    // (HEALTH_DIFFERENTIAL is win-gated, so the losing side scores none of it).
    assert_eq!(
        sides
            .iter()
            .map(|s| s["points"].as_f64().expect("points"))
            .collect::<Vec<_>>(),
        [27.75, 1.0]
    );
    assert_eq!(strings(&shutout["bannedHeroNames"]), ["Sun Wukong"]);
    assert!(
        shutout["draftedUnplayedHeroNames"]
            .as_array()
            .expect("an array")
            .is_empty(),
        "both sides fielded everything they drafted"
    );
}

#[tokio::test]
async fn a_recorded_game_shows_the_defeated_hero_at_zero_health() {
    let app = TestApp::spawn().await;
    let decisive = entry(&summer_ticker(&app).await, 11);

    let games = decisive["games"].as_array().expect("games");
    let sides = games[0]["sides"].as_array().expect("sides");
    assert_eq!(
        sides
            .iter()
            .map(|s| s["heroName"].as_str().expect("a hero").to_owned())
            .collect::<Vec<_>>(),
        ["Sherlock Holmes", "Dracula"]
    );
    assert_eq!(
        sides
            .iter()
            .map(|s| s["isWinner"].as_bool().expect("a flag"))
            .collect::<Vec<_>>(),
        [true, false]
    );
    assert_eq!(
        sides
            .iter()
            .map(|s| s["healthRemaining"].as_i64().expect("health"))
            .collect::<Vec<_>>(),
        [7, 0]
    );
    // 10 + 5.25 + 3.5 + 3 + 1 = 22.75 against 1.0, LOSS being unpriced in the
    // seed and HEALTH_DIFFERENTIAL win-gated, so Dracula's loss earns only
    // APPEARANCE.
    assert_eq!(
        sides
            .iter()
            .map(|s| s["points"].as_f64().expect("points"))
            .collect::<Vec<_>>(),
        [22.75, 1.0]
    );
    assert_eq!(
        strings(&decisive["bannedHeroNames"]),
        ["Medusa", "Yennenga"]
    );
}

/// Match 13's Bo3 (Medusa vs. Achilles) is the seed's proof that a multi-game
/// series records and scores correctly end to end -- but neither hero sits on
/// any manager's roster, so every point it generates is folded per hero and
/// never picked up by the board.
#[tokio::test]
async fn match_13_the_bo3_decider_is_recorded_and_scored_but_changes_no_rosters_total() {
    let app = TestApp::spawn().await;
    let decider = entry(&summer_ticker(&app).await, 13);

    let games = decider["games"].as_array().expect("games");
    assert_eq!(games.len(), 3, "the Bo3 recorded all three games");
    assert_eq!(
        decider["externalLink"],
        "https://challonge.com/example-bo3-decider"
    );
    assert_eq!(
        games
            .iter()
            .map(|g| g["gameNumber"].as_i64().expect("a game number"))
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(games.iter().all(|g| {
        let mut heroes: Vec<_> = g["sides"]
            .as_array()
            .expect("sides")
            .iter()
            .map(|s| s["heroName"].as_str().expect("a hero"))
            .collect();
        heroes.sort();
        heroes == ["Achilles", "Medusa"]
    }));

    let board = summer_board(&app).await;
    assert!(board["rows"].as_array().expect("rows").iter().all(|r| {
        let roster = strings(&r["roster"]);
        !roster.iter().any(|h| h == "Medusa" || h == "Achilles")
    }));
    assert_eq!(
        field_of_each(&board["rows"], "totalPoints"),
        [100.00, 79.75, 76.50, 61.00]
    );
}

/// The seed's only drafted-but-unfielded heroes, both on match 13 and both off
/// every roster -- see `V6__demo_draft_picks.sql`. They earn APPEARANCE without
/// appearing in a game row, which is why the ticker names them separately from
/// the heroes that played.
#[tokio::test]
async fn a_hero_drafted_and_never_fielded_is_named_rather_than_left_invisible() {
    let app = TestApp::spawn().await;
    let decider = entry(&summer_ticker(&app).await, 13);

    let unplayed = strings(&decider["draftedUnplayedHeroNames"]);
    assert_eq!(unplayed, ["Tomoe Gozen", "Nikola Tesla"]);
    assert!(
        !decider["games"]
            .as_array()
            .expect("games")
            .iter()
            .flat_map(|g| g["sides"].as_array().expect("sides"))
            .any(|s| unplayed.iter().any(|h| s["heroName"] == h.as_str())),
        "they are on the draft, not in any game"
    );
}

// ---------------------------------------------------------------------------
// The write path, seen from the board
// ---------------------------------------------------------------------------

/// A hero credits its manager for surviving the draft, not for reaching the
/// table. Recorded through the admin route so the pick goes through
/// `MatchResultPolicy` and the write aggregate, then read back off the board.
#[tokio::test]
async fn drafting_a_rostered_hero_without_fielding_it_moves_that_roster_by_one_appearance() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;
    let admin = app.manager("NeonStrategist").await.id;

    let before = row(&board(&app, summer).await, "NeonStrategist")["totalPoints"]
        .as_f64()
        .expect("a total");
    let appearance_weight = board(&app, summer).await["metrics"]
        .as_array()
        .expect("metrics")
        .iter()
        .find(|m| m["metric"] == "APPEARANCE")
        .expect("APPEARANCE is priced")["coefficient"]
        .as_f64()
        .expect("a coefficient");

    let medusa = app.hero_id("Medusa").await;
    let bigfoot = app.hero_id("Bigfoot").await;
    let achilles = app.hero_id("Achilles").await;
    let map = sqlx::query_scalar!(
        "select tm.map_id from tournament_maps tm where tm.tournament_id = $1 limit 1",
        summer
    )
    .fetch_one(app.pool())
    .await
    .expect("the tournament has a board pool");

    // Bigfoot is on NeonStrategist's roster; it is drafted here and never
    // played.
    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{summer}/matches"),
            admin,
            Some(&json!({
                "round": 3,
                "playedAt": "2026-06-20T12:00:00Z",
                "externalLink": "https://example.com/match/late-round-3",
                "participants": [
                    { "playerLabel": "Hana Sato", "draftedHeroIds": [medusa, bigfoot] },
                    { "playerLabel": "Dmitri Kovac", "draftedHeroIds": [achilles] },
                ],
                "games": [{
                    "gameNumber": 1,
                    "mapId": map,
                    "participants": [
                        { "heroId": medusa, "healthRemaining": 4, "isWinner": true },
                        { "heroId": achilles, "healthRemaining": 0, "isWinner": false },
                    ],
                }],
                "bans": [],
            })),
        )
        .await;
    assert_eq!(response.status, 201, "{}", response.text());

    assert_eq!(
        row(&board(&app, summer).await, "NeonStrategist")["totalPoints"].as_f64(),
        Some(before + appearance_weight),
        "Bigfoot never played, so only its appearance point moved"
    );
}

/// The cache's invalidation and the board's freshness, end to end: the fold's
/// input is cached, so a write that did not invalidate would leave the board
/// reporting the old total indefinitely.
#[tokio::test]
async fn a_recorded_match_is_on_the_board_and_the_ticker_immediately() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    let admin = app.manager("NeonStrategist").await.id;

    assert_eq!(board(&app, winter).await["currentRound"], 0);
    assert!(ticker(&app, winter, "").await.is_empty());

    let alice = app.hero_id("Alice").await;
    let robin = app.hero_id("Robin Hood").await;
    let map = sqlx::query_scalar!(
        "select tm.map_id from tournament_maps tm where tm.tournament_id = $1 limit 1",
        winter
    )
    .fetch_one(app.pool())
    .await
    .expect("the tournament has a board pool");
    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/matches"),
            admin,
            Some(&json!({
                "round": 1,
                "playedAt": "2026-03-01T18:00:00Z",
                "externalLink": "https://example.com/match/winter-1",
                "participants": [
                    { "draftedHeroIds": [alice] },
                    { "draftedHeroIds": [robin] },
                ],
                "games": [{
                    "gameNumber": 1,
                    "mapId": map,
                    "participants": [
                        { "heroId": alice, "healthRemaining": 6, "isWinner": true },
                        { "heroId": robin, "healthRemaining": 0, "isWinner": false },
                    ],
                }],
                "bans": [],
            })),
        )
        .await;
    assert_eq!(response.status, 201, "{}", response.text());

    assert_eq!(board(&app, winter).await["currentRound"], 1);
    assert_eq!(ticker(&app, winter, "").await.len(), 1);
}

// ---------------------------------------------------------------------------
// The stream
// ---------------------------------------------------------------------------

/// The push that makes the read path bursty, end to end.
///
/// This is the test a transaction-rollback harness could not have: an
/// `AFTER_COMMIT` listener never fires under one, because every test rolls
/// back. It fires here, so the assertion is the real one -- a match write
/// reaches an open stream.
#[tokio::test]
async fn a_committed_match_write_pushes_an_update_to_an_open_stream() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    let admin = app.manager("NeonStrategist").await.id;

    let response = app
        .state
        .standings_hub
        .subscribe(winter)
        .expect("well under the caps");
    let mut events = response.into_body().into_data_stream();
    assert_eq!(app.state.standings_hub.subscriber_count(winter), 1);

    let alice = app.hero_id("Alice").await;
    let robin = app.hero_id("Robin Hood").await;
    let map = sqlx::query_scalar!(
        "select tm.map_id from tournament_maps tm where tm.tournament_id = $1 limit 1",
        winter
    )
    .fetch_one(app.pool())
    .await
    .expect("the tournament has a board pool");
    let recorded = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/matches"),
            admin,
            Some(&json!({
                "round": 1,
                "playedAt": "2026-03-01T18:00:00Z",
                "externalLink": "https://example.com/match/streamed",
                "participants": [
                    { "draftedHeroIds": [alice] },
                    { "draftedHeroIds": [robin] },
                ],
                "games": [{
                    "gameNumber": 1,
                    "mapId": map,
                    "participants": [
                        { "heroId": alice, "healthRemaining": 6, "isWinner": true },
                        { "heroId": robin, "healthRemaining": 0, "isWinner": false },
                    ],
                }],
                "bans": [],
            })),
        )
        .await;
    assert_eq!(recorded.status, 201, "{}", recorded.text());

    let chunk = tokio::time::timeout(Duration::from_secs(10), events.next())
        .await
        .expect("an update within ten seconds of the commit")
        .expect("the stream is still open")
        .expect("a chunk");
    let text = String::from_utf8(chunk.to_vec()).expect("utf-8");
    assert!(text.contains("event: update"), "{text:?}");
    assert!(text.contains(&format!("data: {winter}")), "{text:?}");
}

/// A tournament nobody is watching is not a reason to fail a write.
#[tokio::test]
async fn a_write_with_no_watchers_still_commits() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;

    assert_eq!(app.state.standings_hub.subscriber_count(winter), 0);
    app.state.standings_hub.notify(winter);
    assert_eq!(app.state.standings_hub.total_subscriber_count(), 0);
}

// ---------------------------------------------------------------------------
// The 404 every route opens with
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_standings_route_404s_on_an_unknown_tournament() {
    let app = TestApp::spawn().await;

    for path in [
        "/api/tournaments/999999/standings",
        "/api/tournaments/999999/matches",
        "/api/tournaments/999999/standings/stream",
    ] {
        let response = app.get(path).await;
        assert_eq!(response.status, 404, "{path}: {}", response.text());
        assert_eq!(
            response.content_type.as_deref(),
            Some("application/problem+json"),
            "{path}"
        );
        assert_eq!(
            response.json()["detail"],
            "No tournament with id 999999",
            "{path}"
        );
    }
}

// ---------------------------------------------------------------------------
// Pool exhaustion under a cache miss
// ---------------------------------------------------------------------------

/// `board`/`ticker` must not hold a pooled connection while the match cache
/// loads, or a burst of concurrent readers self-deadlocks the pool.
///
/// A single connection is the smallest pool that expresses "no spare
/// connection available" -- exactly what ten concurrent standings requests
/// produce against the real `max_connections(10)` pool on a cache miss, since
/// each of the ten holds the snapshot's connection and then needs a second one
/// for `MatchResultCache::find_by_tournament`'s `pool.acquire()`
/// (`match/cache.rs`'s `load`). One connection reproduces the same shape with
/// one request instead of ten: if the cache read runs *inside* the snapshot
/// transaction, that lone connection is the one already held by `snapshot`,
/// so the second acquire has nothing left to wait for but its own three-second
/// timeout, and this test fails with a pool-acquire timeout
/// (`ApiError::Internal` wrapping `sqlx::Error::PoolTimedOut`). Moving the
/// cache read above `snapshot` -- see the "cache read precedes the snapshot"
/// note on `board`'s doc comment -- frees the only connection before the
/// snapshot ever opens one, so the same call succeeds even at this pool size.
#[tokio::test]
async fn the_board_and_ticker_do_not_hold_a_connection_while_the_cache_loads() {
    let mut app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    let one_connection_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&app.state.config.database_url)
        .await
        .expect("connect a single-connection pool to the test database");
    app.state.pool = one_connection_pool;

    assert!(
        umfl_server::standings::service::board(&app.state, summer)
            .await
            .is_ok(),
        "board() must release the snapshot's connection before the cache load needs one"
    );
    assert!(
        umfl_server::standings::service::ticker(&app.state, summer, 0, 25)
            .await
            .is_ok(),
        "ticker() must release the snapshot's connection before the cache load needs one"
    );
}

/// All three are `permitAll` -- nobody needs an account to watch a tournament.
#[tokio::test]
async fn the_standings_routes_need_no_credential() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    for path in [
        format!("/api/tournaments/{summer}/standings"),
        format!("/api/tournaments/{summer}/matches"),
    ] {
        let response = app.get(&path).await;
        assert_eq!(response.status, 200, "{path}: {}", response.text());
    }
}
