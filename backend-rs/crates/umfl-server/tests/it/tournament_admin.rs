//! Tournament create/update/delete, through `/api/admin/tournaments`.
//!
//! Oracle: `tournament/AdminTournamentServiceIntegrationTest.kt`, driven
//! over HTTP rather than against the service, so the DTO shape and status
//! codes are checked by the same test that checks the rule -- see
//! `map_admin.rs` for the same treatment of `AdminMapServiceIntegrationTest`.

use serde_json::json;

use crate::harness::TestApp;

/// *NeonStrategist* is the seed's only `is_admin` manager.
async fn admin(app: &TestApp) -> i64 {
    app.manager("NeonStrategist").await.id
}

fn valid_body(name: &str) -> serde_json::Value {
    json!({
        "name": name,
        "format": "ARSENAL",
        "status": "SCHEDULED",
        "startDate": "2026-10-01",
        "capacity": 32,
        "rosterSize": 3,
        "creditGrant": 10_000,
    })
}

async fn entry_count(app: &TestApp, tournament_id: i64) -> i64 {
    sqlx::query_scalar!(
        r#"select count(*) as "count!" from tournament_entries where tournament_id = $1"#,
        tournament_id
    )
    .fetch_one(app.pool())
    .await
    .expect("count tournament_entries")
}

async fn match_count(app: &TestApp, tournament_id: i64) -> i64 {
    sqlx::query_scalar!(
        r#"select count(*) as "count!" from tournament_matches where tournament_id = $1"#,
        tournament_id
    )
    .fetch_one(app.pool())
    .await
    .expect("count tournament_matches")
}

async fn hero_pool_count(app: &TestApp, tournament_id: i64) -> i64 {
    sqlx::query_scalar!(
        r#"select count(*) as "count!" from tournament_heroes where tournament_id = $1"#,
        tournament_id
    )
    .fetch_one(app.pool())
    .await
    .expect("count tournament_heroes")
}

async fn map_pool_count(app: &TestApp, tournament_id: i64) -> i64 {
    sqlx::query_scalar!(
        r#"select count(*) as "count!" from tournament_maps where tournament_id = $1"#,
        tournament_id
    )
    .fetch_one(app.pool())
    .await
    .expect("count tournament_maps")
}

async fn rule_set_count(app: &TestApp, tournament_id: i64) -> i64 {
    sqlx::query_scalar!(
        r#"select count(*) as "count!" from scoring_rule_sets where tournament_id = $1"#,
        tournament_id
    )
    .fetch_one(app.pool())
    .await
    .expect("count scoring_rule_sets")
}

// ---------------------------------------------------------------------------
// Create
// ---------------------------------------------------------------------------

#[tokio::test]
async fn creates_a_new_tournament() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&valid_body("Autumn Invitational")),
        )
        .await;

    assert_eq!(response.status, 201, "{}", response.text());
    response.assert_no_json_nulls();
    let body = response.json();
    assert!(body["id"].as_i64().is_some(), "{body}");
    assert_eq!(body["name"], "Autumn Invitational");
    assert_eq!(body["format"], "ARSENAL");
    assert_eq!(body["status"], "SCHEDULED");
    assert_eq!(body["startDate"], "2026-10-01");
    assert_eq!(body["capacity"], 32);
    assert_eq!(body["rosterSize"], 3);
    assert_eq!(body["creditGrant"], 10_000);
    assert_eq!(body["enrolled"], 0);
    assert_eq!(body["acceptsRegistration"], false);
    assert!(
        body.get("myEntryStatus").is_none(),
        "the admin never has an entry status here"
    );

    assert_eq!(app.tournament_id("Autumn Invitational").await, body["id"]);
}

#[tokio::test]
async fn created_tournament_is_visible_on_the_public_lobby() {
    let app = TestApp::spawn().await;

    app.send_as(
        "POST",
        "/api/admin/tournaments",
        admin(&app).await,
        Some(&valid_body("Newly Announced Cup")),
    )
    .await;

    let response = app.get("/api/tournaments").await;
    let body = response.json();
    let names: Vec<&str> = body
        .as_array()
        .expect("an array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"Newly Announced Cup"), "{names:?}");
}

#[tokio::test]
async fn creating_a_tournament_with_a_name_already_in_use_is_rejected() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&valid_body("Winter of Champions")),
        )
        .await;

    assert_eq!(response.status, 409, "{}", response.text());
    let body = response.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/conflict");
    assert_eq!(
        body["detail"],
        "A tournament named 'Winter of Champions' already exists."
    );
}

#[tokio::test]
async fn a_blank_tournament_name_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;
    let body = valid_body("   ");

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    let body = response.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/validation-failed");
    assert_eq!(body["fields"]["name"], "name is required");
}

#[tokio::test]
async fn a_missing_format_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;
    let mut body = valid_body("Missing Format Cup");
    body.as_object_mut().unwrap().remove("format");

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(response.json()["fields"]["format"], "format is required");
}

#[tokio::test]
async fn a_missing_status_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;
    let mut body = valid_body("Missing Status Cup");
    body.as_object_mut().unwrap().remove("status");

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(response.json()["fields"]["status"], "status is required");
}

#[tokio::test]
async fn a_missing_start_date_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;
    let mut body = valid_body("Missing Start Date Cup");
    body.as_object_mut().unwrap().remove("startDate");

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(
        response.json()["fields"]["startDate"],
        "startDate is required"
    );
}

/// PORTING.md deviation (a): `capacity`/`rosterSize`/`creditGrant` carry
/// `@Positive` but no `@NotNull` in the Kotlin, which 500s there today. This
/// is a 400 naming the field with Hibernate's own default `@NotNull`
/// message, since no custom one was ever attached to the (absent)
/// annotation.
#[tokio::test]
async fn a_missing_capacity_is_a_400_not_a_500() {
    let app = TestApp::spawn().await;
    let mut body = valid_body("Missing Capacity Cup");
    body.as_object_mut().unwrap().remove("capacity");

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(response.json()["fields"]["capacity"], "must not be null");
}

#[tokio::test]
async fn a_non_positive_capacity_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;
    let mut body = valid_body("Non Positive Capacity Cup");
    body["capacity"] = json!(0);

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(
        response.json()["fields"]["capacity"],
        "capacity must be positive"
    );
}

#[tokio::test]
async fn a_missing_roster_size_is_a_400_not_a_500() {
    let app = TestApp::spawn().await;
    let mut body = valid_body("Missing Roster Size Cup");
    body.as_object_mut().unwrap().remove("rosterSize");

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(response.json()["fields"]["rosterSize"], "must not be null");
}

#[tokio::test]
async fn a_missing_credit_grant_is_a_400_not_a_500() {
    let app = TestApp::spawn().await;
    let mut body = valid_body("Missing Credit Grant Cup");
    body.as_object_mut().unwrap().remove("creditGrant");

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(response.json()["fields"]["creditGrant"], "must not be null");
}

#[tokio::test]
async fn end_date_may_be_omitted() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&valid_body("Open Ended Cup")),
        )
        .await;

    assert_eq!(response.status, 201, "{}", response.text());
    response.assert_no_json_nulls();
    assert!(response.json().get("endDate").is_none());
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

#[tokio::test]
async fn updates_an_existing_tournament_including_moving_it_to_the_next_lifecycle_status() {
    let app = TestApp::spawn().await;
    let spring = app.tournament_id("Spring of Myths").await;

    let mut body = valid_body("Spring of Myths");
    body["status"] = json!("REGISTRATION_OPEN");
    body["capacity"] = json!(40);

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{spring}"),
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    let updated = response.json();
    assert_eq!(updated["status"], "REGISTRATION_OPEN");
    assert_eq!(updated["capacity"], 40);
    assert_eq!(updated["acceptsRegistration"], true);

    let reread = app.get(&format!("/api/tournaments/{spring}")).await.json();
    assert_eq!(reread["status"], "REGISTRATION_OPEN");
    assert_eq!(reread["capacity"], 40);
}

#[tokio::test]
async fn update_echoes_the_current_enrolment_count() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let before = app.get(&format!("/api/tournaments/{winter}")).await.json()["enrolled"]
        .as_i64()
        .expect("an enrolled count");

    let mut body = valid_body("Winter of Champions");
    body["status"] = json!("REGISTRATION_OPEN");

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{winter}"),
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["enrolled"], before);
}

#[tokio::test]
async fn renaming_a_tournament_to_a_name_already_in_use_is_rejected() {
    let app = TestApp::spawn().await;
    let spring = app.tournament_id("Spring of Myths").await;

    let body = valid_body("Winter of Champions");

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{spring}"),
            admin(&app).await,
            Some(&body),
        )
        .await;

    assert_eq!(response.status, 409, "{}", response.text());
    assert_eq!(
        response.json()["detail"],
        "A tournament named 'Winter of Champions' already exists."
    );
}

#[tokio::test]
async fn renaming_a_tournament_to_its_own_name_is_not_a_collision() {
    let app = TestApp::spawn().await;
    let spring = app.tournament_id("Spring of Myths").await;

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{spring}"),
            admin(&app).await,
            Some(&valid_body("Spring of Myths")),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
}

#[tokio::test]
async fn updating_an_unknown_tournament_is_a_404() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "PUT",
            "/api/admin/tournaments/9999999",
            admin(&app).await,
            Some(&valid_body("Nowhere Cup")),
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["detail"], "No tournament with id 9999999");
}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn deleting_a_tournament_removes_it_and_allows_creating_a_new_one_with_the_same_name() {
    let app = TestApp::spawn().await;
    let spring = app.tournament_id("Spring of Myths").await;

    let delete = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{spring}"),
            admin(&app).await,
            None,
        )
        .await;
    assert_eq!(delete.status, 204, "{}", delete.text());
    assert!(delete.body.is_empty(), "204 carries no body");

    assert_eq!(
        app.get(&format!("/api/tournaments/{spring}")).await.status,
        404
    );

    let recreated = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            admin(&app).await,
            Some(&valid_body("Spring of Myths")),
        )
        .await;
    assert_eq!(recreated.status, 201, "{}", recreated.text());
    assert_ne!(
        recreated.json()["id"],
        spring,
        "should be a new tournament with a new id"
    );
}

#[tokio::test]
async fn deleting_a_tournament_with_related_data_cascades_properly() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id("Summer of Legends").await;

    let entries_before = entry_count(&app, summer).await;
    let matches_before = match_count(&app, summer).await;
    assert!(entries_before > 0, "Summer of Legends should have entries");
    assert!(matches_before > 0, "Summer of Legends should have matches");

    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{summer}"),
            admin(&app).await,
            None,
        )
        .await;
    assert_eq!(response.status, 204, "{}", response.text());

    assert_eq!(entry_count(&app, summer).await, 0);
    assert_eq!(match_count(&app, summer).await, 0);
    assert_eq!(hero_pool_count(&app, summer).await, 0);
    assert_eq!(map_pool_count(&app, summer).await, 0);
    assert_eq!(rule_set_count(&app, summer).await, 0);
}

#[tokio::test]
async fn deleting_an_unknown_tournament_is_a_404() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "DELETE",
            "/api/admin/tournaments/9999999",
            admin(&app).await,
            None,
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["detail"], "No tournament with id 9999999");
}

/// Admin-only, like every route under `/api/admin/**`.
#[tokio::test]
async fn admin_tournament_routes_reject_a_non_admin() {
    let app = TestApp::spawn().await;
    let non_admin = app.manager("MythicMind").await.id;

    let response = app
        .send_as(
            "POST",
            "/api/admin/tournaments",
            non_admin,
            Some(&valid_body("Should Not Exist")),
        )
        .await;

    assert_eq!(response.status, 403, "{}", response.text());
}
