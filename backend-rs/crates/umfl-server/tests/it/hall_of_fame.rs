//! The Hall of Fame, against the seed's one completed tournament.
//!
//! The podium is checked against `/standings` rather than against a second
//! hand-copied table of seed totals: the Hall of Fame is that board's top
//! three ranks and nothing else, so the cross-check is the whole contract --
//! and it keeps holding when the seed is retuned.

use serde_json::{Value, json};

use crate::harness::TestApp;

const SUMMER: &str = "Summer of Legends";

async fn hall_of_fame(app: &TestApp) -> Value {
    let response = app.get("/api/hall-of-fame").await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    response.json()
}

fn names(hall: &Value) -> Vec<&str> {
    hall["tournaments"]
        .as_array()
        .expect("tournaments is an array")
        .iter()
        .map(|t| t["name"].as_str().expect("a name"))
        .collect()
}

#[tokio::test]
async fn only_completed_tournaments_are_listed() {
    let app = TestApp::spawn().await;

    // Winter is open for registration and Spring is scheduled; neither has a
    // winner yet.
    assert_eq!(names(&hall_of_fame(&app).await), [SUMMER]);
}

#[tokio::test]
async fn the_podium_is_the_top_three_ranks_of_the_final_board() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id(SUMMER).await;

    let board = app
        .get(&format!("/api/tournaments/{summer}/standings"))
        .await
        .json();
    let board_rows = board["rows"].as_array().expect("rows");
    let expected: Vec<&Value> = board_rows
        .iter()
        .filter(|r| r["rank"].as_i64().expect("a rank") <= 3)
        .collect();
    assert!(!expected.is_empty(), "the seed records Summer's results");

    let hall = hall_of_fame(&app).await;
    let entry = &hall["tournaments"][0];
    assert_eq!(entry["tournamentId"], summer);
    assert_eq!(entry["ruleSetName"], board["ruleSetName"]);
    assert_eq!(entry["entrants"].as_u64(), Some(board_rows.len() as u64));
    let podium: Vec<&Value> = entry["podium"].as_array().expect("podium").iter().collect();
    assert_eq!(podium, expected);
}

#[tokio::test]
async fn the_champion_holds_a_title() {
    let app = TestApp::spawn().await;
    let hall = hall_of_fame(&app).await;

    let champions: Vec<i64> = hall["tournaments"][0]["podium"]
        .as_array()
        .expect("podium")
        .iter()
        .filter(|r| r["rank"] == 1)
        .map(|r| r["managerId"].as_i64().expect("a manager id"))
        .collect();
    assert!(!champions.is_empty());

    for champion in champions {
        let row = hall["titles"]
            .as_array()
            .expect("titles")
            .iter()
            .find(|t| t["managerId"] == champion)
            .unwrap_or_else(|| panic!("manager {champion} is in the titles table"));
        assert_eq!(row["titles"], 1);
        assert_eq!(row["podiums"], 1);
        assert_eq!(row["rank"], 1);
    }
}

#[tokio::test]
async fn a_newly_completed_tournament_is_listed_first() {
    let app = TestApp::spawn().await;
    let admin = app.manager("NeonStrategist").await.id;

    let created = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin,
            Some(&json!({
                "name": "Autumn Finals",
                "format": "BANQUEST",
                "status": "COMPLETED",
                "startDate": "2026-10-01",
                "endDate": "2026-10-03",
                "capacity": 32,
                "rosterSize": 3,
                "creditGrant": 10_000,
            })),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());

    let hall = hall_of_fame(&app).await;
    assert_eq!(names(&hall), ["Autumn Finals", SUMMER]);
    // Nobody entered it, so it is listed without anybody on its podium.
    assert_eq!(hall["tournaments"][0]["entrants"], 0);
    assert_eq!(hall["tournaments"][0]["podium"], json!([]));
}
