//! The one route that touches the database, and the smoke test for the harness
//! itself.
//!
//! `crates/umfl-server/src/lib.rs`'s own test module already covers routing,
//! both fallbacks and the problem body over a pool that never connects. What it
//! cannot cover is the health endpoint, which is the only route that runs a
//! query -- so it is checked here, where there is a real database, and doubles
//! as the proof that `TestApp::oneshot` reaches the real router through every
//! layer `build_router` installs.

use axum::http::StatusCode;

use crate::harness::TestApp;

#[tokio::test]
async fn health_is_up_against_a_migrated_database() {
    let app = TestApp::spawn().await;
    let response = app.get("/actuator/health").await;

    assert_eq!(StatusCode::OK, response.status);
    assert_eq!(Some("application/json"), response.content_type.as_deref());
    // `groups` names the default health-check groups, reported alongside the
    // status: `{"groups":["liveness","readiness"],"status":"UP"}`.
    assert_eq!(
        serde_json::json!({ "groups": ["liveness", "readiness"], "status": "UP" }),
        response.json()
    );

    // `management.endpoint.health.show-details: when-authorized`: an anonymous
    // caller -- which both compose healthchecks are -- sees no `components`.
    assert!(response.json().get("components").is_none());
    response.assert_no_json_nulls();
}

/// Each test gets its *own* database, not a shared one: the whole reason this
/// harness can commit rather than roll back.
#[tokio::test]
async fn a_test_owns_its_database() {
    let app = TestApp::spawn().await;
    let current: String = sqlx::query_scalar("select current_database()")
        .fetch_one(app.pool())
        .await
        .expect("current_database()");

    assert_eq!(app.db_name, current);
    assert!(current.starts_with("umfl_test_"), "{current}");
}
