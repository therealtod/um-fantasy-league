//! Boards and board pools, through `/api/admin/maps` and
//! `/api/admin/tournaments/{id}/maps`.
//!
//! Oracle: `map/AdminMapServiceIntegrationTest.kt`, driven over HTTP rather
//! than against the service, so the DTO shape and the status codes are checked
//! by the same test that checks the rule.
//!
//! The seed's board pools are asserted exactly, as `AGENTS.md` requires:
//! *Summer of Legends* plays on Baskerville Manor, Sherwood Forest and Raptor
//! Paddock and has recorded games; *Winter of Champions* plays on the first two
//! only, which is what makes Raptor Paddock the board every "add to the pool"
//! case below starts from.

use serde_json::{Value, json};

use crate::harness::TestApp;

/// *NeonStrategist* is the seed's only `is_admin` manager.
async fn admin(app: &TestApp) -> i64 {
    app.manager("NeonStrategist").await.id
}

async fn map_id(app: &TestApp, name: &str) -> i64 {
    sqlx::query_scalar!("select id from game_maps where name = $1", name)
        .fetch_one(app.pool())
        .await
        .unwrap_or_else(|e| panic!("no board {name}: {e}"))
}

/// The pool as the database holds it -- the equivalent of the Kotlin's
/// `mapPoolAdminRepository.poolMapIds(...)`, read directly rather than through
/// the endpoint so a pool assertion cannot be satisfied by a broken read path.
async fn pool_map_ids(app: &TestApp, tournament_id: i64) -> Vec<i64> {
    sqlx::query_scalar!(
        "select map_id from tournament_maps where tournament_id = $1",
        tournament_id
    )
    .fetch_all(app.pool())
    .await
    .expect("read the board pool")
}

async fn create_map(app: &TestApp, name: &str) -> Value {
    let response = app
        .send_as(
            "POST",
            "/api/admin/maps",
            admin(app).await,
            Some(&json!({ "name": name })),
        )
        .await;
    assert_eq!(response.status, 201, "{}", response.text());
    response.json()
}

async fn add_to_pool(app: &TestApp, tournament_id: i64, board: i64) -> u16 {
    app.send_as(
        "PUT",
        &format!("/api/admin/tournaments/{tournament_id}/maps/{board}"),
        admin(app).await,
        None,
    )
    .await
    .status
    .as_u16()
}

// ---------------------------------------------------------------------------
// The catalogue
// ---------------------------------------------------------------------------

#[tokio::test]
async fn creates_a_new_map() {
    let app = TestApp::spawn().await;

    let created = create_map(&app, "Sunken Ruins").await;

    assert_eq!(created["name"], "Sunken Ruins");
    assert!(created["id"].as_i64().is_some(), "{created}");
    assert_eq!(map_id(&app, "Sunken Ruins").await, created["id"]);
}

#[tokio::test]
async fn creating_a_map_with_a_name_already_in_use_is_rejected() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "POST",
            "/api/admin/maps",
            admin(&app).await,
            Some(&json!({ "name": "Baskerville Manor" })),
        )
        .await;

    assert_eq!(response.status, 409, "{}", response.text());
    let body = response.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/conflict");
    assert_eq!(
        body["detail"],
        "A map named 'Baskerville Manor' already exists."
    );
}

#[tokio::test]
async fn renames_an_existing_map() {
    let app = TestApp::spawn().await;
    let created = create_map(&app, "Placeholder Arena").await;
    let id = created["id"].as_i64().expect("an id");

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/maps/{id}"),
            admin(&app).await,
            Some(&json!({ "name": "Renamed Arena" })),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    assert_eq!(response.json()["name"], "Renamed Arena");
    assert_eq!(map_id(&app, "Renamed Arena").await, id);
}

/// The collision check exempts the row being renamed, so re-saving a board
/// under its own name is a no-op rather than a 409 against itself.
#[tokio::test]
async fn renaming_a_map_to_its_own_name_is_not_a_collision() {
    let app = TestApp::spawn().await;
    let board = map_id(&app, "Raptor Paddock").await;

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/maps/{board}"),
            admin(&app).await,
            Some(&json!({ "name": "Raptor Paddock" })),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
}

#[tokio::test]
async fn renaming_a_map_onto_another_boards_name_is_rejected() {
    let app = TestApp::spawn().await;
    let board = map_id(&app, "Raptor Paddock").await;

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/maps/{board}"),
            admin(&app).await,
            Some(&json!({ "name": "Sherwood Forest" })),
        )
        .await;

    assert_eq!(response.status, 409, "{}", response.text());
    assert_eq!(
        response.json()["detail"],
        "A map named 'Sherwood Forest' already exists."
    );
}

#[tokio::test]
async fn renaming_an_unknown_map_is_a_404() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "PUT",
            "/api/admin/maps/9999999",
            admin(&app).await,
            Some(&json!({ "name": "Nowhere" })),
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["detail"], "No map with id 9999999");
}

/// `@NotBlank(message = "name is required")` -- whitespace fails it, and the
/// message is the one the client renders.
#[tokio::test]
async fn a_blank_map_name_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;

    let response = app
        .send_as(
            "POST",
            "/api/admin/maps",
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
async fn lists_every_board_in_the_catalogue() {
    let app = TestApp::spawn().await;

    let response = app.get_as("/api/admin/maps", admin(&app).await).await;

    assert_eq!(response.status, 200);
    response.assert_no_json_nulls();
    let body = response.json();
    let listed = body.as_array().expect("an array");
    let names: Vec<&str> = listed.iter().filter_map(|m| m["name"].as_str()).collect();
    assert!(names.contains(&"Baskerville Manor"), "{names:?}");
    assert!(names.contains(&"Raptor Paddock"), "{names:?}");
    // Every row is `{ id, name }` and nothing else -- the DTO, not the entity.
    assert_eq!(listed[0].as_object().expect("an object").len(), 2);
}

// ---------------------------------------------------------------------------
// The pool
// ---------------------------------------------------------------------------

#[tokio::test]
async fn adds_a_map_to_a_tournaments_board_pool() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let raptor_paddock = map_id(&app, "Raptor Paddock").await;
    assert!(
        !pool_map_ids(&app, winter).await.contains(&raptor_paddock),
        "precondition: not yet in Winter's pool"
    );

    assert_eq!(add_to_pool(&app, winter, raptor_paddock).await, 200);

    assert!(pool_map_ids(&app, winter).await.contains(&raptor_paddock));
}

#[tokio::test]
async fn adding_the_same_map_to_a_pool_twice_is_idempotent() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let raptor_paddock = map_id(&app, "Raptor Paddock").await;

    add_to_pool(&app, winter, raptor_paddock).await;
    add_to_pool(&app, winter, raptor_paddock).await;

    let occurrences = pool_map_ids(&app, winter)
        .await
        .iter()
        .filter(|id| **id == raptor_paddock)
        .count();
    assert_eq!(occurrences, 1);
}

#[tokio::test]
async fn the_pool_listing_reflects_a_newly_added_map_by_name() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let raptor_paddock = map_id(&app, "Raptor Paddock").await;
    let pool_uri = format!("/api/admin/tournaments/{winter}/maps");

    let before = app.get_as(&pool_uri, admin(&app).await).await.json();
    assert!(!listed_names(&before).contains(&"Raptor Paddock".to_owned()));

    add_to_pool(&app, winter, raptor_paddock).await;

    let after = app.get_as(&pool_uri, admin(&app).await).await;
    assert_eq!(after.status, 200);
    after.assert_no_json_nulls();
    // `order by gm.name`, so the pool listing is alphabetical.
    assert_eq!(
        listed_names(&after.json()),
        ["Baskerville Manor", "Raptor Paddock", "Sherwood Forest"]
    );
}

fn listed_names(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("an array")
        .iter()
        .filter_map(|m| m["name"].as_str().map(str::to_owned))
        .collect()
}

/// **No `require_tournament`**, exactly as `AdminMapController.listPool` has
/// none: an unknown tournament is an empty pool, not a 404.
#[tokio::test]
async fn the_pool_of_an_unknown_tournament_is_empty_rather_than_a_404() {
    let app = TestApp::spawn().await;

    let response = app
        .get_as("/api/admin/tournaments/9999999/maps", admin(&app).await)
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json(), json!([]));
}

#[tokio::test]
async fn adding_to_the_pool_of_an_unknown_tournament_is_a_404() {
    let app = TestApp::spawn().await;
    let raptor_paddock = map_id(&app, "Raptor Paddock").await;

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/9999999/maps/{raptor_paddock}"),
            admin(&app).await,
            None,
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["detail"], "No tournament with id 9999999");
}

#[tokio::test]
async fn add_batch_to_pool_adds_several_new_maps_in_one_call() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let raptor_paddock = map_id(&app, "Raptor Paddock").await;
    let new_map = create_map(&app, "Batch Map A").await["id"]
        .as_i64()
        .expect("an id");

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/maps"),
            admin(&app).await,
            Some(&json!({ "mapIds": [raptor_paddock, new_map] })),
        )
        .await;

    // 200, not 201 -- `addBatchToPool` carries no `@ResponseStatus`.
    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    assert_eq!(response.json().as_array().expect("an array").len(), 2);

    let pool = pool_map_ids(&app, winter).await;
    assert!(pool.contains(&raptor_paddock));
    assert!(pool.contains(&new_map));
}

#[tokio::test]
async fn add_batch_to_pool_is_idempotent_for_a_map_already_in_the_pool() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let raptor_paddock = map_id(&app, "Raptor Paddock").await;
    add_to_pool(&app, winter, raptor_paddock).await;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/maps"),
            admin(&app).await,
            Some(&json!({ "mapIds": [raptor_paddock, raptor_paddock] })),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    // `mapIds.toSet()` -- the duplicate is collapsed before the lookup, so the
    // echoed list names the board once.
    assert_eq!(response.json().as_array().expect("an array").len(), 1);
    let occurrences = pool_map_ids(&app, winter)
        .await
        .iter()
        .filter(|id| **id == raptor_paddock)
        .count();
    assert_eq!(occurrences, 1);
}

/// One unknown id fails the whole call: the admin picked from a list, so an id
/// that is not there means the list is stale, and adding the rest silently
/// would hide that.
#[tokio::test]
async fn add_batch_to_pool_rejects_an_unknown_map_id() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let raptor_paddock = map_id(&app, "Raptor Paddock").await;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/maps"),
            admin(&app).await,
            Some(&json!({ "mapIds": [raptor_paddock, 9_999_999] })),
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["detail"], "No map(s) with id(s) 9999999");
    assert!(
        !pool_map_ids(&app, winter).await.contains(&raptor_paddock),
        "the whole batch rolls back, including the ids that did resolve"
    );
}

/// `@Size(min = 1, max = 64)` -- and the message the client renders.
#[tokio::test]
async fn an_empty_batch_is_a_400_naming_the_field() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/maps"),
            admin(&app).await,
            Some(&json!({ "mapIds": [] })),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(
        response.json()["fields"]["mapIds"],
        "mapIds must contain between 1 and 64 entries"
    );
}

/// `@Size` ignores a null and there is no `@NotNull` beside it, so an omitted
/// `mapIds` is a valid request that adds nothing. Ported because it is live
/// behaviour, not because anything sends it.
#[tokio::test]
async fn an_omitted_batch_is_accepted_and_adds_nothing() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let before = pool_map_ids(&app, winter).await.len();

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/maps"),
            admin(&app).await,
            Some(&json!({})),
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json(), json!([]));
    assert_eq!(pool_map_ids(&app, winter).await.len(), before);
}

// ---------------------------------------------------------------------------
// Removal, and the deferred foreign key
// ---------------------------------------------------------------------------

#[tokio::test]
async fn removes_a_map_from_a_tournaments_pool() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let raptor_paddock = map_id(&app, "Raptor Paddock").await;
    add_to_pool(&app, winter, raptor_paddock).await;

    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{winter}/maps/{raptor_paddock}"),
            admin(&app).await,
            None,
        )
        .await;

    assert_eq!(response.status, 204, "{}", response.text());
    assert!(response.body.is_empty(), "204 carries no body");
    assert!(!pool_map_ids(&app, winter).await.contains(&raptor_paddock));
}

#[tokio::test]
async fn removing_a_map_not_in_the_pool_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let created = create_map(&app, "Never Pooled Arena").await["id"]
        .as_i64()
        .expect("an id");

    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{winter}/maps/{created}"),
            admin(&app).await,
            None,
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(
        response.json()["detail"],
        format!("Map {created} is not in tournament {winter}'s pool")
    );
}

/// The asymmetry with the hero pool, and the reason for it: `match_games`
/// carries a composite FK onto `tournament_maps`, so a board with a recorded
/// game on it cannot leave the pool. Dropping a *hero* is always allowed and
/// simply re-prices the rosters still holding it.
#[tokio::test]
async fn removing_a_map_with_a_recorded_match_on_it_is_rejected_rather_than_a_raw_fk_error() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id("Summer of Legends").await;
    let baskerville_manor = map_id(&app, "Baskerville Manor").await;

    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{summer}/maps/{baskerville_manor}"),
            admin(&app).await,
            None,
        )
        .await;

    assert_eq!(response.status, 409, "{}", response.text());
    let body = response.json();
    // The named conflict, not the generic data-integrity backstop.
    assert_eq!(body["type"], "https://umfl.dev/problems/conflict");
    assert_eq!(
        body["detail"],
        format!(
            "Map {baskerville_manor} has recorded matches in tournament {summer} \
             and cannot be removed from its pool."
        )
    );
    assert!(
        pool_map_ids(&app, summer)
            .await
            .contains(&baskerville_manor),
        "the pool row must survive the rejected removal"
    );
}

/// The pool FK is checked on the spot despite being deferrable, so the race
/// past the pre-check still fails inside the service.
///
/// Going straight at the link-table writes is what a game recorded between the
/// service's pre-check and its delete looks like from here: the delete itself
/// succeeds, and only the FK stands between it and an orphaned game. Deferred,
/// that FK would not speak up until `COMMIT` -- past the point where
/// `admin_service::remove_from_pool` could still name the board.
#[tokio::test]
async fn the_pool_fk_is_checked_on_the_spot_despite_being_deferrable() {
    use umfl_server::map::pool_admin;

    let app = TestApp::spawn().await;
    let summer = app.tournament_id("Summer of Legends").await;
    let baskerville_manor = map_id(&app, "Baskerville Manor").await;

    let mut tx = app.pool().begin().await.expect("begin");
    let removed = pool_admin::remove_from_pool(&mut *tx, summer, baskerville_manor)
        .await
        .expect("the delete itself succeeds -- the FK is deferred");
    assert!(removed);

    let err = pool_admin::check_map_in_pool_now(&mut tx)
        .await
        .expect_err("the deferred FK fires here, not at commit");
    let sqlx::Error::Database(db) = &err else {
        panic!("expected a database error, got {err:?}");
    };
    assert!(db.is_foreign_key_violation(), "{err:?}");
    assert_eq!(db.constraint(), Some("match_game_map_in_pool"));
}
