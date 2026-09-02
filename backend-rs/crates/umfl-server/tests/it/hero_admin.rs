//! Hero identities and per-tournament hero pools, through
//! `/api/admin/heroes` and `/api/admin/tournaments/{id}/heroes`.
//!
//! Oracle: `hero/AdminHeroServiceIntegrationTest.kt`, driven over HTTP rather
//! than against the service, so the DTO shape and the status codes are
//! checked by the same test that checks the rule -- see `map_admin.rs` for
//! the same treatment of `AdminMapServiceIntegrationTest`.

use serde_json::json;

use crate::harness::TestApp;

/// *NeonStrategist* is the seed's only `is_admin` manager.
async fn admin(app: &TestApp) -> i64 {
    app.manager("NeonStrategist").await.id
}

async fn hero_cost(app: &TestApp, tournament_id: i64, hero_id: i64) -> Option<i32> {
    sqlx::query_scalar!(
        "select cost from tournament_heroes where tournament_id = $1 and hero_id = $2",
        tournament_id,
        hero_id
    )
    .fetch_optional(app.pool())
    .await
    .expect("read the hero pool")
}

async fn in_pool(app: &TestApp, tournament_id: i64, hero_id: i64) -> bool {
    hero_cost(app, tournament_id, hero_id).await.is_some()
}

async fn create_hero(app: &TestApp, name: &str) -> serde_json::Value {
    let response = app
        .send_as(
            "POST",
            "/api/admin/heroes",
            admin(app).await,
            Some(&json!({ "name": name })),
        )
        .await;
    assert_eq!(response.status, 201, "{}", response.text());
    response.json()
}

// ---------------------------------------------------------------------------
// The catalogue
// ---------------------------------------------------------------------------

#[tokio::test]
async fn creates_a_new_hero() {
    let app = TestApp::spawn().await;

    let created = create_hero(&app, "Achilles' Shield").await;

    assert_eq!(created["name"], "Achilles' Shield");
    assert!(created["id"].as_i64().is_some(), "{created}");
    assert!(
        created.get("imageUrl").is_none(),
        "null image_url is absent, not null"
    );
}

#[tokio::test]
async fn creates_a_hero_with_artwork() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "POST",
            "/api/admin/heroes",
            admin(&app).await,
            Some(&json!({ "name": "Painted Hero", "imageUrl": "https://example.test/art.png" })),
        )
        .await;

    assert_eq!(response.status, 201, "{}", response.text());
    response.assert_no_json_nulls();
    assert_eq!(response.json()["imageUrl"], "https://example.test/art.png");
}

#[tokio::test]
async fn creating_a_hero_with_a_name_already_in_use_is_rejected() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "POST",
            "/api/admin/heroes",
            admin(&app).await,
            Some(&json!({ "name": "Alice" })),
        )
        .await;

    assert_eq!(response.status, 409, "{}", response.text());
    let body = response.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/conflict");
    assert_eq!(body["detail"], "A hero named 'Alice' already exists.");
}

#[tokio::test]
async fn updates_an_existing_heros_name_and_artwork() {
    let app = TestApp::spawn().await;
    let created = create_hero(&app, "Placeholder Hero").await;
    let id = created["id"].as_i64().expect("an id");

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/heroes/{id}"),
            admin(&app).await,
            Some(&json!({ "name": "Renamed Hero", "imageUrl": "https://example.test/art.png" })),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    let body = response.json();
    assert_eq!(body["name"], "Renamed Hero");
    assert_eq!(body["imageUrl"], "https://example.test/art.png");
}

#[tokio::test]
async fn renaming_a_hero_to_its_own_name_is_not_a_collision() {
    let app = TestApp::spawn().await;
    let bigfoot = app.hero_id("Bigfoot").await;

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/heroes/{bigfoot}"),
            admin(&app).await,
            Some(&json!({ "name": "Bigfoot" })),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
}

#[tokio::test]
async fn renaming_a_hero_onto_another_heros_name_is_rejected() {
    let app = TestApp::spawn().await;
    let bigfoot = app.hero_id("Bigfoot").await;

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/heroes/{bigfoot}"),
            admin(&app).await,
            Some(&json!({ "name": "Alice" })),
        )
        .await;

    assert_eq!(response.status, 409, "{}", response.text());
    assert_eq!(
        response.json()["detail"],
        "A hero named 'Alice' already exists."
    );
}

#[tokio::test]
async fn updating_an_unknown_hero_is_a_404() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "PUT",
            "/api/admin/heroes/9999999",
            admin(&app).await,
            Some(&json!({ "name": "Nowhere Hero" })),
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["detail"], "No hero with id 9999999");
}

/// `@NotBlank(message = "name is required")` -- whitespace fails it, and the
/// message is the one the client renders.
#[tokio::test]
async fn a_blank_hero_name_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "POST",
            "/api/admin/heroes",
            admin(&app).await,
            Some(&json!({ "name": "   " })),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    let body = response.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/validation-failed");
    assert_eq!(body["fields"]["name"], "name is required");
}

#[tokio::test]
async fn lists_every_hero_in_the_catalogue() {
    let app = TestApp::spawn().await;

    let response = app.get_as("/api/admin/heroes", admin(&app).await).await;

    assert_eq!(response.status, 200);
    response.assert_no_json_nulls();
    let body = response.json();
    let listed = body.as_array().expect("an array");
    let names: Vec<&str> = listed.iter().filter_map(|h| h["name"].as_str()).collect();
    assert!(names.contains(&"Bigfoot"), "{names:?}");
    assert!(names.contains(&"Alice"), "{names:?}");
    // Every row is `{ id, name }` and (only when present) `imageUrl` -- the
    // admin DTO, not the entity.
    let row = listed[0].as_object().expect("an object");
    assert!(row.len() <= 3, "{row:?}");
}

// ---------------------------------------------------------------------------
// The pool and its pricing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn adds_a_brand_new_hero_to_a_tournaments_pool_at_the_given_cost() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let hero = create_hero(&app, "Fresh Recruit").await["id"]
        .as_i64()
        .expect("an id");

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{winter}/heroes/{hero}"),
            admin(&app).await,
            Some(&json!({ "cost": 3_300 })),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    assert_eq!(response.json()["cost"], 3_300);
    assert_eq!(hero_cost(&app, winter, hero).await, Some(3_300));
}

/// Re-pricing through the admin path re-prices an unlocked roster, the same
/// as the raw SQL path does -- "no cost snapshot" is the invariant, not a
/// mechanism-specific behaviour.
#[tokio::test]
async fn re_pricing_a_hero_repricing_an_unlocked_roster_holding_it() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let bigfoot = app.hero_id("Bigfoot").await;
    assert_eq!(hero_cost(&app, winter, bigfoot).await, Some(2_100));

    let mythic = app.manager("MythicMind").await.id;
    app.send_as(
        "POST",
        &format!("/api/tournaments/{winter}/entries"),
        mythic,
        None,
    )
    .await;
    let drafted = app
        .send_as(
            "PUT",
            &format!("/api/tournaments/{winter}/entries/me/slots"),
            mythic,
            Some(&json!({ "heroIds": [bigfoot] })),
        )
        .await;
    assert_eq!(drafted.json()["budget"]["spent"], 2_100);

    let repriced = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{winter}/heroes/{bigfoot}"),
            admin(&app).await,
            Some(&json!({ "cost": 3_000 })),
        )
        .await;
    assert_eq!(repriced.json()["cost"], 3_000);

    let reloaded = app
        .get_as(&format!("/api/tournaments/{winter}/entries/me"), mythic)
        .await;
    assert_eq!(
        reloaded.json()["budget"]["spent"],
        3_000,
        "nothing was snapshotted, so the draft re-prices itself"
    );
}

#[tokio::test]
async fn setting_the_cost_of_an_unknown_hero_is_a_404() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{winter}/heroes/9999999"),
            admin(&app).await,
            Some(&json!({ "cost": 1_000 })),
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["detail"], "No hero with id 9999999");
}

/// PORTING.md deviation (a)'s shape, ported to the hero pool's own
/// `@Positive`-without-`@NotNull` `cost` field: an absent cost is a 400
/// naming the field, not a 500.
#[tokio::test]
async fn an_absent_cost_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let bigfoot = app.hero_id("Bigfoot").await;

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{winter}/heroes/{bigfoot}"),
            admin(&app).await,
            Some(&json!({})),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(response.json()["fields"]["cost"], "must not be null");
}

#[tokio::test]
async fn a_non_positive_cost_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let bigfoot = app.hero_id("Bigfoot").await;

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{winter}/heroes/{bigfoot}"),
            admin(&app).await,
            Some(&json!({ "cost": 0 })),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(response.json()["fields"]["cost"], "cost must be positive");
}

#[tokio::test]
async fn add_batch_to_pool_adds_several_brand_new_heroes_in_one_call() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let hero_a = create_hero(&app, "Batch Hero A").await["id"]
        .as_i64()
        .expect("an id");
    let hero_b = create_hero(&app, "Batch Hero B").await["id"]
        .as_i64()
        .expect("an id");

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/heroes"),
            admin(&app).await,
            Some(&json!({ "heroes": [
                { "heroId": hero_a, "cost": 1_200 },
                { "heroId": hero_b, "cost": 2_400 },
            ] })),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    let costs: Vec<i64> = response
        .json()
        .as_array()
        .expect("an array")
        .iter()
        .map(|h| h["cost"].as_i64().unwrap())
        .collect();
    assert_eq!(costs.len(), 2);
    assert!(costs.contains(&1_200));
    assert!(costs.contains(&2_400));
    assert_eq!(hero_cost(&app, winter, hero_a).await, Some(1_200));
    assert_eq!(hero_cost(&app, winter, hero_b).await, Some(2_400));
}

#[tokio::test]
async fn add_batch_to_pool_reprices_a_hero_already_in_the_pool_alongside_adding_a_new_one() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let bigfoot = app.hero_id("Bigfoot").await;
    let new_hero = create_hero(&app, "Batch Hero C").await["id"]
        .as_i64()
        .expect("an id");

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/heroes"),
            admin(&app).await,
            Some(&json!({ "heroes": [
                { "heroId": bigfoot, "cost": 3_500 },
                { "heroId": new_hero, "cost": 1_000 },
            ] })),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(hero_cost(&app, winter, bigfoot).await, Some(3_500));
    assert_eq!(hero_cost(&app, winter, new_hero).await, Some(1_000));
}

#[tokio::test]
async fn add_batch_to_pool_dedupes_a_repeated_hero_id_last_cost_wins() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let new_hero = create_hero(&app, "Batch Hero E").await["id"]
        .as_i64()
        .expect("an id");

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/heroes"),
            admin(&app).await,
            Some(&json!({ "heroes": [
                { "heroId": new_hero, "cost": 1_000 },
                { "heroId": new_hero, "cost": 2_000 },
            ] })),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    let listed = response.json();
    let listed = listed.as_array().expect("an array");
    assert_eq!(listed.len(), 1, "the duplicate collapses to one echoed row");
    assert_eq!(listed[0]["cost"], 2_000);
    assert_eq!(hero_cost(&app, winter, new_hero).await, Some(2_000));
}

#[tokio::test]
async fn add_batch_to_pool_rejects_an_unknown_hero_id_and_writes_nothing_from_the_batch() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let new_hero = create_hero(&app, "Batch Hero D").await["id"]
        .as_i64()
        .expect("an id");

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/heroes"),
            admin(&app).await,
            Some(&json!({ "heroes": [
                { "heroId": new_hero, "cost": 1_500 },
                { "heroId": 9_999_999, "cost": 1_500 },
            ] })),
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["detail"], "No hero(es) with id(s) 9999999");
    assert!(
        !in_pool(&app, winter, new_hero).await,
        "the whole batch rolls back, including the valid entry"
    );
}

/// `@Size(min = 1, max = 128)` -- and the message the client renders.
#[tokio::test]
async fn an_empty_batch_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/heroes"),
            admin(&app).await,
            Some(&json!({ "heroes": [] })),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(
        response.json()["fields"]["heroes"],
        "heroes must contain between 1 and 128 entries"
    );
}

/// `@Size` ignores a null and there is no `@NotNull` beside it, so an
/// omitted `heroes` is a valid request that adds nothing.
#[tokio::test]
async fn an_omitted_batch_is_accepted_and_adds_nothing() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/heroes"),
            admin(&app).await,
            Some(&json!({})),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json(), json!([]));
}

#[tokio::test]
async fn pool_lists_a_tournaments_hero_pool_admin_scoped() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .get_as(
            &format!("/api/admin/tournaments/{winter}/heroes"),
            admin(&app).await,
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    let body = response.json();
    let names: Vec<&str> = body
        .as_array()
        .expect("an array")
        .iter()
        .filter_map(|h| h["name"].as_str())
        .collect();
    assert!(names.contains(&"Bigfoot"), "{names:?}");
}

// ---------------------------------------------------------------------------
// Removal
// ---------------------------------------------------------------------------

#[tokio::test]
async fn removes_a_hero_from_a_tournaments_pool() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let bigfoot = app.hero_id("Bigfoot").await;

    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{winter}/heroes/{bigfoot}"),
            admin(&app).await,
            None,
        )
        .await;

    assert_eq!(response.status, 204, "{}", response.text());
    assert!(response.body.is_empty(), "204 carries no body");
    assert!(!in_pool(&app, winter, bigfoot).await);
}

#[tokio::test]
async fn removing_a_hero_not_in_the_pool_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let hero = create_hero(&app, "Never Pooled").await["id"]
        .as_i64()
        .expect("an id");

    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{winter}/heroes/{hero}"),
            admin(&app).await,
            None,
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(
        response.json()["detail"],
        format!("Hero {hero} is not in tournament {winter}'s pool")
    );
}

/// The asymmetry with the board pool, and the reason for it: dropping a
/// *hero* is always allowed and simply re-prices the rosters still holding
/// it to 0, whereas dropping a *board* with a recorded game on it is
/// rejected (see `map_admin.rs`'s equivalent test).
#[tokio::test]
async fn removing_a_hero_from_the_pool_reprices_an_unlocked_roster_holding_it_to_zero() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let bigfoot = app.hero_id("Bigfoot").await;
    let mythic = app.manager("MythicMind").await.id;

    app.send_as(
        "POST",
        &format!("/api/tournaments/{winter}/entries"),
        mythic,
        None,
    )
    .await;
    let drafted = app
        .send_as(
            "PUT",
            &format!("/api/tournaments/{winter}/entries/me/slots"),
            mythic,
            Some(&json!({ "heroIds": [bigfoot] })),
        )
        .await;
    assert_eq!(drafted.json()["budget"]["spent"], 2_100);

    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{winter}/heroes/{bigfoot}"),
            admin(&app).await,
            None,
        )
        .await;
    assert_eq!(response.status, 204, "{}", response.text());

    let reloaded = app
        .get_as(&format!("/api/tournaments/{winter}/entries/me"), mythic)
        .await;
    assert_eq!(
        reloaded.json()["budget"]["spent"],
        0,
        "the slot still holds the hero, but it costs nothing once out of the pool"
    );
}

/// Admin-only, like every route under `/api/admin/**`.
#[tokio::test]
async fn admin_hero_routes_reject_a_non_admin() {
    let app = TestApp::spawn().await;
    let non_admin = app.manager("MythicMind").await.id;

    let response = app
        .send_as(
            "POST",
            "/api/admin/heroes",
            non_admin,
            Some(&json!({ "name": "Should Not Exist" })),
        )
        .await;

    assert_eq!(response.status, 403, "{}", response.text());
}
