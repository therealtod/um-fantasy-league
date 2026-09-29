//! Hero performance: every pool hero ranked per scoring criterion, against the
//! recorded results in the seed.
//!
//! Rather than hand-derive a second table of seed numbers, the assertions here
//! lean on the leaderboard the `standings` tests already pin exactly: the seed
//! has no roster swaps, so a manager's total is precisely the sum of their
//! roster heroes' overall points. That cross-check is what catches the two
//! folds drifting apart.

use std::collections::HashMap;

use serde_json::{Value, json};

use crate::harness::TestApp;

const SUMMER: &str = "Summer of Legends";
const WINTER: &str = "Winter of Champions";

async fn hero_stats(app: &TestApp, tournament_id: i64) -> Value {
    let response = app
        .get(&format!("/api/tournaments/{tournament_id}/hero-stats"))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    response.json()
}

async fn standings(app: &TestApp, tournament_id: i64) -> Value {
    let response = app
        .get(&format!("/api/tournaments/{tournament_id}/standings"))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.json()
}

fn rows(table: &Value) -> &Vec<Value> {
    table.as_array().expect("a table is an array of rows")
}

fn points_by_hero(table: &Value) -> HashMap<String, f64> {
    rows(table)
        .iter()
        .map(|r| {
            (
                r["heroName"].as_str().expect("a name").to_owned(),
                r["points"].as_f64().expect("points"),
            )
        })
        .collect()
}

fn cents(value: f64) -> i64 {
    (value * 100.0).round() as i64
}

#[tokio::test]
async fn there_is_one_table_per_leaderboard_column_in_the_same_order() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    let stats = hero_stats(&app, summer).await;
    let board = standings(&app, summer).await;

    assert_eq!(stats["tournamentId"].as_i64(), Some(summer));
    assert_eq!(stats["ruleSetName"], board["ruleSetName"]);
    assert_eq!(stats["currentRound"], board["currentRound"]);

    let table_metrics: Vec<&Value> = stats["categories"]
        .as_array()
        .expect("categories")
        .iter()
        .map(|c| &c["metric"])
        .collect();
    let column_metrics: Vec<&Value> = board["metrics"]
        .as_array()
        .expect("metrics")
        .iter()
        .map(|m| &m["metric"])
        .collect();
    assert_eq!(table_metrics, column_metrics);
    assert!(
        !table_metrics.iter().any(|m| *m == "CROWD_FAVOURITE"),
        "weighted in the seed but unimplemented, so it gets no table"
    );
}

#[tokio::test]
async fn every_pool_hero_is_ranked_in_every_table() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    let pool = app
        .get(&format!("/api/tournaments/{summer}/heroes"))
        .await
        .json();
    let mut pool_names: Vec<String> = rows(&pool)
        .iter()
        .map(|h| h["name"].as_str().expect("a name").to_owned())
        .collect();
    pool_names.sort();

    let stats = hero_stats(&app, summer).await;
    let tables = std::iter::once(&stats["overall"]).chain(
        stats["categories"]
            .as_array()
            .expect("categories")
            .iter()
            .map(|c| &c["rows"]),
    );
    for table in tables {
        let mut names: Vec<String> = points_by_hero(table).into_keys().collect();
        names.sort();
        assert_eq!(names, pool_names, "exactly the pool, in every table");

        // Descending, with standard competition ranks.
        let rows = rows(table);
        for (index, pair) in rows.windows(2).enumerate() {
            let (a, b) = (&pair[0], &pair[1]);
            assert!(a["points"].as_f64() >= b["points"].as_f64());
            let expected = if a["points"] == b["points"] {
                a["rank"].as_i64()
            } else {
                Some(index as i64 + 2)
            };
            assert_eq!(b["rank"].as_i64(), expected, "{b}");
        }
        assert_eq!(rows[0]["rank"], 1);
        assert!(
            rows.iter().all(|r| r["cost"].is_i64()),
            "every row is a pool hero, so every row carries its price"
        );
    }
}

/// The seed has no swaps, so each manager held their roster all tournament
/// and their total is exactly the sum of those heroes' overall lines.
#[tokio::test]
async fn a_managers_total_is_the_sum_of_their_heroes_overall_points() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    let overall = points_by_hero(&hero_stats(&app, summer).await["overall"]);
    let board = standings(&app, summer).await;

    for row in rows(&board["rows"]) {
        let summed: f64 = row["roster"]
            .as_array()
            .expect("a roster")
            .iter()
            .map(|name| overall[name.as_str().expect("a name")])
            .sum();
        assert_eq!(
            cents(summed),
            cents(row["totalPoints"].as_f64().expect("a total")),
            "{}",
            row["handle"]
        );
    }
}

#[tokio::test]
async fn an_unknown_tournament_is_a_404() {
    let app = TestApp::spawn().await;
    let response = app.get("/api/tournaments/999999/hero-stats").await;
    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/problem+json")
    );
}

/// The fold's input is cached; a write that did not reach this route would
/// leave the tables frozen on the old results.
#[tokio::test]
async fn a_recorded_match_shows_up_in_the_tables_immediately() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    let admin = app.manager("NeonStrategist").await.id;

    let before = hero_stats(&app, winter).await;
    assert_eq!(before["currentRound"], 0);
    assert!(rows(&before["overall"]).iter().all(|r| r["points"] == 0.0));

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
                "externalLink": "https://example.com/match/winter-hero-stats",
                "participants": [
                    { "draftedHeroIds": [alice] },
                    { "draftedHeroIds": [robin] },
                ],
                "games": [{
                    "gameNumber": 1,
                    "mapId": map,
                    "participants": [
                        { "heroId": alice, "healthRemaining": 5, "isWinner": true },
                        { "heroId": robin, "healthRemaining": 0, "isWinner": false },
                    ],
                }],
                "bans": [],
            })),
        )
        .await;
    assert_eq!(response.status, 201, "{}", response.text());

    let after = hero_stats(&app, winter).await;
    assert_eq!(after["currentRound"], 1);
    assert_eq!(rows(&after["overall"])[0]["heroName"], "Alice");
    let win = after["categories"]
        .as_array()
        .expect("categories")
        .iter()
        .find(|c| c["metric"] == "WIN")
        .expect("Winter prices WIN");
    assert_eq!(rows(&win["rows"])[0]["heroName"], "Alice");
    assert!(rows(&win["rows"])[0]["points"].as_f64() > Some(0.0));
}
