//! Roster roles end to end: the admin defining them, a manager assigning them,
//! a recorded match carrying per-game stats, and the board pricing the role
//! bonus off both.
//!
//! The seed defines Winter of Champions' three roles -- Attacker (ATTACKS
//! x1.0, DAMAGE_DEALT x0.25), Healer (HEALING x1.5, at most one per roster)
//! and Tactician (SCHEMES_PLAYED x2.0) -- but leaves `roles_enabled` off, so
//! every test here that needs the mechanic switches it on first.

use serde_json::{Value, json};
use umfl_domain::DomainError;
use umfl_server::error::ApiError;
use umfl_server::manager::Manager;
use umfl_server::tournament::{admin_service as tournament_admin_service, service};

use crate::harness::TestApp;

const WINTER: &str = "Winter of Champions";

/// The three heroes every roster here drafts; they fit Winter's grant.
const PICKS: [&str; 3] = ["Sherlock Holmes", "Yennenga", "Sinbad"];

fn rules(err: &ApiError) -> Vec<String> {
    match err {
        ApiError::Domain(e @ DomainError::RosterRule(_)) => {
            e.violations().iter().map(|v| v.rule.clone()).collect()
        }
        other => panic!("expected a roster-rule 422, got {other:?}"),
    }
}

fn violation_codes(response: &Value) -> Vec<String> {
    response["violations"]
        .as_array()
        .expect("violations")
        .iter()
        .map(|v| v["rule"].as_str().expect("rule").to_owned())
        .collect()
}

async fn enable_roles(app: &TestApp, tournament_id: i64, on: bool) {
    sqlx::query!(
        "update tournaments set roles_enabled = $2 where id = $1",
        tournament_id,
        on
    )
    .execute(app.pool())
    .await
    .expect("toggle roles");
}

async fn role_id(app: &TestApp, tournament_id: i64, name: &str) -> i64 {
    sqlx::query_scalar!(
        "select id from roster_roles where tournament_id = $1 and name = $2",
        tournament_id,
        name
    )
    .fetch_one(app.pool())
    .await
    .unwrap_or_else(|e| panic!("no role {name}: {e}"))
}

async fn any_map(app: &TestApp, tournament_id: i64) -> i64 {
    sqlx::query_scalar!(
        "select map_id from tournament_maps where tournament_id = $1 order by map_id limit 1",
        tournament_id
    )
    .fetch_one(app.pool())
    .await
    .expect("the tournament has a board pool")
}

/// Attacker, Healer, Tactician -- in [`PICKS`] order.
async fn standard_roles(app: &TestApp, winter: i64, picks: &[i64]) -> Vec<(i64, i64)> {
    vec![
        (picks[0], role_id(app, winter, "Attacker").await),
        (picks[1], role_id(app, winter, "Healer").await),
        (picks[2], role_id(app, winter, "Tactician").await),
    ]
}

/// Registered and drafted, but not yet given roles or locked.
async fn drafted(app: &TestApp, winter: i64, manager: &Manager) -> Vec<i64> {
    service::register(&app.state, winter, manager)
        .await
        .unwrap();
    let picks = app.hero_ids(&PICKS).await;
    service::set_slots(&app.state, winter, manager, &picks)
        .await
        .unwrap();
    picks
}

/// Drafted, given the standard roles and locked.
async fn locked_with_roles(app: &TestApp, winter: i64, manager: &Manager) -> Vec<i64> {
    let picks = drafted(app, winter, manager).await;
    let roles = standard_roles(app, winter, &picks).await;
    service::set_roles(&app.state, winter, manager, &roles)
        .await
        .unwrap();
    service::lock_roster(&app.state, winter, manager)
        .await
        .unwrap();
    picks
}

async fn role_log(app: &TestApp, winter: i64, manager_id: i64) -> Vec<(i64, i32, i64)> {
    sqlx::query!(
        "select r.hero_id, r.from_round, r.role_id
           from entry_hero_roles r
           join tournament_entries e on e.id = r.entry_id
          where e.tournament_id = $1 and e.manager_id = $2
          order by r.from_round, r.hero_id",
        winter,
        manager_id
    )
    .fetch_all(app.pool())
    .await
    .unwrap()
    .into_iter()
    .map(|r| (r.hero_id, r.from_round, r.role_id))
    .collect()
}

// ---------------------------------------------------------------------------
// Definitions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_roles_are_public_and_carry_their_weights() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;

    let response = app.get(&format!("/api/tournaments/{winter}/roles")).await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    let roles = response.json();

    let names: Vec<&str> = roles
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Attacker", "Healer", "Tactician"], "sort order");

    assert!(
        roles[0].get("maxPerRoster").is_none(),
        "uncapped is absent, not null"
    );
    assert_eq!(roles[1]["maxPerRoster"], 1);
    assert_eq!(
        roles[0]["weights"],
        json!([
            { "stat": "ATTACKS", "coefficient": 1.0 },
            { "stat": "DAMAGE_DEALT", "coefficient": 0.25 },
        ])
    );
}

#[tokio::test]
async fn an_admin_creates_updates_and_deletes_a_role() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    let admin = app.manager("NeonStrategist").await.id;
    let base = format!("/api/admin/tournaments/{winter}/roles");

    let created = app
        .send_as(
            "POST",
            &base,
            admin,
            Some(&json!({
                "name": "Defender",
                "maxPerRoster": 2,
                "sortOrder": 4,
                "weights": [{ "stat": " blocks ", "coefficient": 1.25 }],
            })),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let created = created.json();
    assert_eq!(created["weights"][0]["stat"], "BLOCKS", "stored normalised");
    let id = created["id"].as_i64().unwrap();

    let updated = app
        .send_as(
            "PUT",
            &format!("{base}/{id}"),
            admin,
            Some(&json!({
                "name": "Guardian",
                "weights": [
                    { "stat": "BLOCKS", "coefficient": 1.0 },
                    { "stat": "DEFENCE_PLAYED", "coefficient": 0.5 },
                ],
            })),
        )
        .await;
    assert_eq!(updated.status, 200, "{}", updated.text());
    let updated = updated.json();
    assert_eq!(updated["name"], "Guardian");
    assert!(
        updated.get("maxPerRoster").is_none(),
        "a full replace: an omitted cap is uncapped"
    );
    assert_eq!(updated["weights"].as_array().unwrap().len(), 2);

    let deleted = app
        .send_as("DELETE", &format!("{base}/{id}"), admin, None)
        .await;
    assert_eq!(deleted.status, 204, "{}", deleted.text());
    let listed = app.get(&format!("/api/tournaments/{winter}/roles")).await;
    assert_eq!(listed.json().as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn a_bad_role_definition_is_named_rather_than_a_generic_conflict() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    let admin = app.manager("NeonStrategist").await.id;
    let base = format!("/api/admin/tournaments/{winter}/roles");

    let malformed = app
        .send_as(
            "POST",
            &base,
            admin,
            Some(&json!({
                "name": "Sloppy",
                "weights": [
                    { "stat": "damage-dealt", "coefficient": 1 },
                    { "stat": "Attacks", "coefficient": 1 },
                    { "stat": "ATTACKS", "coefficient": 2 },
                ],
            })),
        )
        .await;
    assert_eq!(malformed.status, 422, "{}", malformed.text());
    assert_eq!(
        violation_codes(&malformed.json()),
        ["MALFORMED_STAT", "DUPLICATE_STAT"]
    );

    let taken = app
        .send_as("POST", &base, admin, Some(&json!({ "name": "Healer" })))
        .await;
    assert_eq!(taken.status, 409, "{}", taken.text());

    let zero_cap = app
        .send_as(
            "POST",
            &base,
            admin,
            Some(&json!({ "name": "Nobody", "maxPerRoster": 0 })),
        )
        .await;
    assert_eq!(zero_cap.status, 400, "{}", zero_cap.text());
}

#[tokio::test]
async fn role_admin_is_admin_only() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    let manager = app.manager("SherlockMain").await.id;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/roles"),
            manager,
            Some(&json!({ "name": "Sneaky" })),
        )
        .await;
    assert_eq!(response.status, 403);
}

#[tokio::test]
async fn a_role_somebody_assigned_cannot_be_deleted() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    enable_roles(&app, winter, true).await;
    let manager = app.manager("SherlockMain").await;
    let picks = drafted(&app, winter, &manager).await;
    let healer = role_id(&app, winter, "Healer").await;
    service::set_roles(&app.state, winter, &manager, &[(picks[1], healer)])
        .await
        .unwrap();

    let admin = app.manager("NeonStrategist").await.id;
    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{winter}/roles/{healer}"),
            admin,
            None,
        )
        .await;
    assert_eq!(response.status, 409, "{}", response.text());
    assert!(
        response.text().contains("Healer"),
        "the message names the role: {}",
        response.text()
    );
}

// ---------------------------------------------------------------------------
// Assigning
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roles_are_refused_where_the_tournament_does_not_use_them() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    let manager = app.manager("SherlockMain").await;
    let picks = drafted(&app, winter, &manager).await;
    let attacker = role_id(&app, winter, "Attacker").await;

    let err = service::set_roles(&app.state, winter, &manager, &[(picks[0], attacker)])
        .await
        .expect_err("roles are off");
    assert_eq!(rules(&err), ["ROLES_DISABLED"]);

    let snapshot = service::lock_roster(&app.state, winter, &manager)
        .await
        .expect("with roles off, a roster without them locks");
    assert!(snapshot.role_assignments.is_empty());
}

#[tokio::test]
async fn a_draft_may_go_without_roles_but_cannot_lock_without_them() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    enable_roles(&app, winter, true).await;
    let manager = app.manager("SherlockMain").await;
    let picks = drafted(&app, winter, &manager).await;
    let attacker = role_id(&app, winter, "Attacker").await;

    let partial = service::set_roles(&app.state, winter, &manager, &[(picks[0], attacker)])
        .await
        .expect("a draft is a scratchpad");
    assert_eq!(partial.role_assignments, [(picks[0], attacker)]);
    assert!(!partial.lockable, "two heroes still have no role");

    let err = service::lock_roster(&app.state, winter, &manager)
        .await
        .expect_err("two heroes have no role");
    assert_eq!(rules(&err), ["ROLE_UNASSIGNED"]);

    let roles = standard_roles(&app, winter, &picks).await;
    let complete = service::set_roles(&app.state, winter, &manager, &roles)
        .await
        .unwrap();
    assert!(complete.lockable);
    service::lock_roster(&app.state, winter, &manager)
        .await
        .expect("fully assigned");

    // Every draft-time row sits at round one: there is no history yet.
    assert!(
        role_log(&app, winter, manager.id)
            .await
            .iter()
            .all(|&(_, from_round, _)| from_round == 1)
    );
}

#[tokio::test]
async fn a_draft_may_run_a_role_over_its_cap_but_cannot_lock_that_way() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    enable_roles(&app, winter, true).await;
    let manager = app.manager("SherlockMain").await;
    let picks = drafted(&app, winter, &manager).await;
    let healer = role_id(&app, winter, "Healer").await;
    let tactician = role_id(&app, winter, "Tactician").await;
    let two_healers = [
        (picks[0], healer),
        (picks[1], healer),
        (picks[2], tactician),
    ];

    // The scratchpad lets Healer move straight from one hero to the next...
    let snapshot = service::set_roles(&app.state, winter, &manager, &two_healers)
        .await
        .expect("a draft is a scratchpad");
    assert_eq!(snapshot.role_assignments, two_healers);

    // ...but the cap holds where it counts.
    let err = service::lock_roster(&app.state, winter, &manager)
        .await
        .expect_err("Healer is capped at one");
    assert_eq!(rules(&err), ["ROLE_CAP_EXCEEDED"]);
}

#[tokio::test]
async fn a_locked_roster_cannot_run_a_role_over_its_cap() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    enable_roles(&app, winter, true).await;
    let manager = app.manager("SherlockMain").await;
    let picks = locked_with_roles(&app, winter, &manager).await;
    let healer = role_id(&app, winter, "Healer").await;
    let tactician = role_id(&app, winter, "Tactician").await;
    tournament_admin_service::advance_round(&app.state, winter)
        .await
        .unwrap();
    tournament_admin_service::set_swap_window(&app.state, winter, true)
        .await
        .unwrap();

    let err = service::set_roles(
        &app.state,
        winter,
        &manager,
        &[
            (picks[0], healer),
            (picks[1], healer),
            (picks[2], tactician),
        ],
    )
    .await
    .expect_err("Healer is capped at one");
    assert_eq!(rules(&err), ["ROLE_CAP_EXCEEDED"]);
}

#[tokio::test]
async fn a_hero_dropped_from_a_draft_takes_its_role_with_it() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    enable_roles(&app, winter, true).await;
    let manager = app.manager("SherlockMain").await;
    let picks = drafted(&app, winter, &manager).await;
    let roles = standard_roles(&app, winter, &picks).await;
    service::set_roles(&app.state, winter, &manager, &roles)
        .await
        .unwrap();

    let snapshot = service::set_slots(&app.state, winter, &manager, &picks[..2])
        .await
        .unwrap();

    assert_eq!(snapshot.role_assignments, roles[..2]);
    assert_eq!(role_log(&app, winter, manager.id).await.len(), 2);
}

#[tokio::test]
async fn a_locked_rosters_roles_change_only_while_a_window_is_open() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    enable_roles(&app, winter, true).await;
    let manager = app.manager("SherlockMain").await;
    let picks = locked_with_roles(&app, winter, &manager).await;
    let attacker = role_id(&app, winter, "Attacker").await;
    let healer = role_id(&app, winter, "Healer").await;
    let tactician = role_id(&app, winter, "Tactician").await;
    let reshuffled = [
        (picks[0], tactician),
        (picks[1], healer),
        (picks[2], attacker),
    ];

    let err = service::set_roles(&app.state, winter, &manager, &reshuffled)
        .await
        .expect_err("the window is shut");
    assert_eq!(rules(&err), ["ROLE_CHANGE_CLOSED"]);

    tournament_admin_service::advance_round(&app.state, winter)
        .await
        .unwrap();
    tournament_admin_service::set_swap_window(&app.state, winter, true)
        .await
        .unwrap();
    let snapshot = service::set_roles(&app.state, winter, &manager, &reshuffled)
        .await
        .expect("the window is open");

    assert_eq!(snapshot.role_assignments, reshuffled);
    assert!(
        !snapshot.already_swapped_this_round,
        "re-assigning roles is not the round's swap submission"
    );
    // Round one keeps the roles it was played under; only the two heroes that
    // changed gain a row from round two.
    let mut expected = vec![
        (picks[0], 1, attacker),
        (picks[1], 1, healer),
        (picks[2], 1, tactician),
        (picks[0], 2, tactician),
        (picks[2], 2, attacker),
    ];
    expected.sort_by_key(|&(hero, round, _)| (round, hero));
    assert_eq!(role_log(&app, winter, manager.id).await, expected);
}

#[tokio::test]
async fn a_hero_arriving_by_swap_needs_a_role() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    enable_roles(&app, winter, true).await;
    sqlx::query!(
        "update tournaments set swaps_per_round = 1 where id = $1",
        winter
    )
    .execute(app.pool())
    .await
    .unwrap();
    let manager = app.manager("SherlockMain").await;
    let picks = locked_with_roles(&app, winter, &manager).await;
    tournament_admin_service::advance_round(&app.state, winter)
        .await
        .unwrap();
    tournament_admin_service::set_swap_window(&app.state, winter, true)
        .await
        .unwrap();

    let beowulf = app.hero_id("Beowulf").await;
    let proposed = [picks[0], picks[1], beowulf];
    let err = service::swap_roster(&app.state, winter, &manager, &proposed, &[])
        .await
        .expect_err("Beowulf arrives without a role");
    assert_eq!(rules(&err), ["ROLE_UNASSIGNED"]);

    let tactician = role_id(&app, winter, "Tactician").await;
    let snapshot = service::swap_roster(
        &app.state,
        winter,
        &manager,
        &proposed,
        &[(beowulf, tactician)],
    )
    .await
    .expect("Beowulf brings a role");

    assert_eq!(snapshot.role_assignments[2], (beowulf, tactician));
    assert!(
        role_log(&app, winter, manager.id)
            .await
            .contains(&(beowulf, 2, tactician)),
        "the arrival's role starts in the round it arrived"
    );
}

#[tokio::test]
async fn the_roles_endpoint_speaks_the_wire_shape() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    enable_roles(&app, winter, true).await;
    let manager = app.manager("SherlockMain").await;
    let picks = drafted(&app, winter, &manager).await;
    let healer = role_id(&app, winter, "Healer").await;
    let uri = format!("/api/tournaments/{winter}/entries/me/roles");

    let response = app
        .send_as(
            "PUT",
            &uri,
            manager.id,
            Some(&json!({ "assignments": [{ "heroId": picks[1], "roleId": healer }] })),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(
        response.json()["roleAssignments"],
        json!([{ "heroId": picks[1], "roleId": healer }])
    );

    let missing = app.send_as("PUT", &uri, manager.id, Some(&json!({}))).await;
    assert_eq!(missing.status, 400, "{}", missing.text());

    let tournament = app.get(&format!("/api/tournaments/{winter}")).await.json();
    assert_eq!(tournament["rolesEnabled"], true);
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

/// Sherlock Holmes beats Yennenga; Sherlock attacked `attacks` times for 8
/// damage, Yennenga healed 3. Under the standard roles, 4 attacks is
/// 4 + 8 * 0.25 = 6.00 for the Attacker, and 3 * 1.5 = 4.50 for the Healer.
fn match_body(map: i64, sherlock: i64, yennenga: i64, attacks: i32) -> Value {
    json!({
        "round": 1,
        "playedAt": "2026-08-20T18:00:00Z",
        "externalLink": "https://example.com/match/winter-roles",
        "participants": [
            { "draftedHeroIds": [sherlock] },
            { "draftedHeroIds": [yennenga] },
        ],
        "games": [{
            "gameNumber": 1,
            "mapId": map,
            "participants": [
                {
                    "heroId": sherlock, "healthRemaining": 5, "isWinner": true,
                    "stats": { "attacks": attacks, "DAMAGE_DEALT": 8 },
                },
                {
                    "heroId": yennenga, "healthRemaining": 0, "isWinner": false,
                    "stats": { "HEALING": 3 },
                },
            ],
        }],
        "bans": [],
    })
}

async fn standings_row(app: &TestApp, winter: i64, handle: &str) -> (Value, Value) {
    let response = app
        .get(&format!("/api/tournaments/{winter}/standings"))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    let board = response.json();
    let row = board["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["handle"] == handle)
        .cloned()
        .unwrap_or_else(|| panic!("no row for {handle}"));
    (board, row)
}

#[tokio::test]
async fn recorded_stats_score_the_role_bonus_and_a_correction_re_prices_it() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    enable_roles(&app, winter, true).await;
    let manager = app.manager("SherlockMain").await;
    let picks = locked_with_roles(&app, winter, &manager).await;
    let admin = app.manager("NeonStrategist").await.id;
    let map = any_map(&app, winter).await;

    let recorded = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/matches"),
            admin,
            Some(&match_body(map, picks[0], picks[1], 4)),
        )
        .await;
    assert_eq!(recorded.status, 201, "{}", recorded.text());
    let recorded = recorded.json();
    assert_eq!(
        recorded["games"][0]["participants"][0]["stats"],
        json!({ "ATTACKS": 4, "DAMAGE_DEALT": 8 }),
        "stats come back normalised"
    );
    let match_id = recorded["matchId"].as_i64().unwrap();

    let (board, row) = standings_row(&app, winter, "SherlockMain").await;
    let last = board["metrics"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["metric"], "ROLE_BONUS");
    assert_eq!(last["label"], "Role Bonus");
    assert_eq!(
        row["breakdown"]["ROLE_BONUS"].as_f64(),
        Some(10.5),
        "6.00 as Attacker + 4.50 as Healer"
    );
    let with_roles = row["totalPoints"].as_f64().unwrap();

    // The admin corrects the attack count; the board re-prices on the next read.
    let corrected = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{winter}/matches/{match_id}"),
            admin,
            Some(&match_body(map, picks[0], picks[1], 6)),
        )
        .await;
    assert_eq!(corrected.status, 200, "{}", corrected.text());
    let (_, row) = standings_row(&app, winter, "SherlockMain").await;
    assert_eq!(row["breakdown"]["ROLE_BONUS"].as_f64(), Some(12.5));
    let corrected_total = row["totalPoints"].as_f64().unwrap();
    assert_eq!(corrected_total, with_roles + 2.0);

    // Switching roles off hides the bonus without deleting anything...
    enable_roles(&app, winter, false).await;
    let (board, row) = standings_row(&app, winter, "SherlockMain").await;
    assert!(
        !board["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["metric"] == "ROLE_BONUS")
    );
    assert!(row["breakdown"].get("ROLE_BONUS").is_none());
    assert_eq!(
        row["totalPoints"].as_f64().unwrap(),
        corrected_total - 12.5,
        "the bonus was the only difference"
    );

    // ...so switching it back on restores it.
    enable_roles(&app, winter, true).await;
    let (_, row) = standings_row(&app, winter, "SherlockMain").await;
    assert_eq!(row["breakdown"]["ROLE_BONUS"].as_f64(), Some(12.5));
}

#[tokio::test]
async fn a_malformed_or_negative_stat_is_a_named_422() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id(WINTER).await;
    let admin = app.manager("NeonStrategist").await.id;
    let heroes = app.hero_ids(&["Sherlock Holmes", "Yennenga"]).await;
    let map = any_map(&app, winter).await;

    let mut body = match_body(map, heroes[0], heroes[1], -1);
    body["games"][0]["participants"][1]["stats"] = json!({ "heal-ing": 3 });
    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/matches"),
            admin,
            Some(&body),
        )
        .await;
    assert_eq!(response.status, 422, "{}", response.text());
    assert_eq!(
        violation_codes(&response.json()),
        ["STAT_NAME_MALFORMED", "STAT_VALUE_NEGATIVE"]
    );
}
