//! The authorization rule table, asserted from outside the process.
//!
//! Oracle: `config/SecurityConfigTest.kt`, `config/DevSecurityConfigTest.kt`
//! and `config/AdminSecurityDevTest.kt` -- three classes because Spring needed
//! a separate application context per profile. Here the profile is a field on
//! `Config`, so the dev-credential half is one file, and the prod half (a
//! signed ES256 token and a JWKS to verify it against) is a unit-level concern
//! rather than something to stand a fake identity provider up for.
//!
//! **These assert the gate, not the routes.** Most feature routes have not
//! landed yet, so a request the table *permits* reaches the router and 404s.
//! That is precisely the distinction worth pinning: 401/403 is the rule table
//! refusing, 404 is the rule table having let go. The Kotlin tests assert
//! `isOk()` where these assert "not a rejection", and they converge as each
//! feature merges.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};

use crate::harness::{TestApp, TestResponse};

const MANAGER_ID: &str = "X-Manager-Id";

/// The seeded managers, by handle rather than by id -- `AGENTS.md` warns the
/// integration tests assert on the seed's numbers exactly, and a handle is the
/// stable half of that.
async fn manager_id(app: &TestApp, handle: &str) -> i64 {
    sqlx::query_scalar!("select id from managers where handle = $1", handle)
        .fetch_one(app.pool())
        .await
        .unwrap_or_else(|e| panic!("no seeded manager {handle}: {e}"))
}

async fn get(app: &TestApp, uri: &str, manager: Option<&str>) -> TestResponse {
    send(app, "GET", uri, manager).await
}

/// [`send`] without collecting the body -- see `TestApp::oneshot_status`.
async fn status_of(app: &TestApp, method: &str, uri: &str, manager: Option<&str>) -> StatusCode {
    app.oneshot_status(request_for(method, uri, manager)).await
}

async fn send(app: &TestApp, method: &str, uri: &str, manager: Option<&str>) -> TestResponse {
    app.oneshot(request_for(method, uri, manager)).await
}

fn request_for(method: &str, uri: &str, manager: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(id) = manager {
        builder = builder.header(MANAGER_ID, id);
    }
    builder
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("well-formed request")
}

/// Viewing a tournament needs no account; only entering one does.
#[tokio::test]
async fn the_public_reads_need_no_credential() {
    let app = TestApp::spawn().await;
    for uri in [
        "/api/tournaments",
        "/api/tournaments/1",
        "/api/tournaments/1/heroes",
        "/api/tournaments/1/standings",
        "/api/tournaments/1/standings/stream",
        "/api/tournaments/1/matches",
        "/actuator/health",
        "/actuator/info",
    ] {
        // The status only, and never the body: one of these is the SSE stream,
        // whose body has no last byte to collect.
        let status = status_of(&app, "GET", uri, None).await;
        assert_ne!(status, StatusCode::UNAUTHORIZED, "GET {uri}");
        assert_ne!(status, StatusCode::FORBIDDEN, "GET {uri}");
    }
}

/// The `permitAll` rules are GET-only. Verified against the running Kotlin
/// backend: `POST /api/tournaments` and `PUT /api/tournaments/1` both 401.
#[tokio::test]
async fn a_write_to_a_publicly_readable_path_still_needs_an_identity() {
    let app = TestApp::spawn().await;
    for (method, uri) in [
        ("POST", "/api/tournaments"),
        ("PUT", "/api/tournaments/1"),
        ("POST", "/api/tournaments/1/entries"),
    ] {
        let response = send(&app, method, uri, None).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[tokio::test]
async fn a_manager_route_without_a_credential_is_a_401_not_an_anonymous_500() {
    let app = TestApp::spawn().await;
    let response = get(&app, "/api/me", None).await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/problem+json")
    );
    assert_eq!(response.json()["title"], "Unauthorized");
}

/// Admin routes are role-gated in dev/test too, purely via `X-Manager-Id` --
/// the whole reason the header is resolved at the filter level rather than in a
/// handler.
#[tokio::test]
async fn the_admin_gate_answers_by_role() {
    let app = TestApp::spawn().await;
    let admin = manager_id(&app, "NeonStrategist").await.to_string();
    let non_admin = manager_id(&app, "SherlockMain").await.to_string();

    // Anonymous: no identity offered at all.
    assert_eq!(
        send(&app, "POST", "/api/admin/heroes", None).await.status,
        StatusCode::UNAUTHORIZED
    );
    // Identified, and it is not enough.
    let forbidden = send(&app, "POST", "/api/admin/heroes", Some(&non_admin)).await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert_eq!(forbidden.json()["title"], "Forbidden");
    // The admin clears the gate; the route itself has not landed yet.
    let allowed = send(&app, "POST", "/api/admin/heroes", Some(&admin)).await;
    assert_ne!(allowed.status, StatusCode::UNAUTHORIZED);
    assert_ne!(allowed.status, StatusCode::FORBIDDEN);
}

/// A credential that is present and unusable is rejected rather than silently
/// downgraded to anonymous -- the same way prod treats a malformed bearer
/// token. All three shapes are one 401 on the wire.
#[tokio::test]
async fn a_bad_credential_is_rejected_rather_than_treated_as_anonymous() {
    let app = TestApp::spawn().await;
    for bad in ["not-a-number", "999999", ""] {
        let response = get(&app, "/api/me", Some(bad)).await;
        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{MANAGER_ID}: {bad}"
        );
        assert_eq!(
            response.content_type.as_deref(),
            Some("application/problem+json"),
            "{MANAGER_ID}: {bad}"
        );
    }
}

/// Resolution is conditional on a credential being **offered**, never on the
/// route -- so a bad one is rejected even where the route needed none.
///
/// This is the outward half of `DevSecurityConfigTest`'s "a public route costs
/// no manager lookup": the filter knows nothing about routes, so it cannot
/// skip a lookup for a public one, and equally cannot run one when no header
/// arrived. Confirmed against the running Kotlin backend, which answers 401 to
/// `GET /api/tournaments` and even `GET /actuator/health` when the header names
/// no manager.
#[tokio::test]
async fn a_bad_credential_is_rejected_even_on_a_public_route() {
    let app = TestApp::spawn().await;
    assert_eq!(
        get(&app, "/api/tournaments", Some("999999")).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&app, "/actuator/health", Some("999999")).await.status,
        StatusCode::UNAUTHORIZED
    );
    // ... and a good one changes nothing about a route that needed none.
    let admin = manager_id(&app, "NeonStrategist").await.to_string();
    assert_eq!(
        get(&app, "/actuator/health", Some(&admin)).await.status,
        StatusCode::OK
    );
}

/// `anyRequest().denyAll()`, and the reason authorization is one middleware
/// over the raw path rather than a per-route layer: a per-route layer never
/// runs for a path matching no route, so axum would 404 where Spring denies.
///
/// The four rows are transcribed from probing the running Kotlin backend.
#[tokio::test]
async fn an_unrouted_path_is_denied_by_the_table_not_by_the_router() {
    let app = TestApp::spawn().await;
    let non_admin = manager_id(&app, "SherlockMain").await.to_string();

    // Outside `/api`, so nothing but `anyRequest()` matches it.
    assert_eq!(
        get(&app, "/nope", None).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&app, "/nope", Some(&non_admin)).await.status,
        StatusCode::FORBIDDEN
    );
    // Under `/api`, so `/api/**` matches and merely demands an identity: an
    // authenticated caller is handed to the router, which answers its own 404.
    assert_eq!(
        get(&app, "/api/nope", None).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&app, "/api/nope", Some(&non_admin)).await.status,
        StatusCode::NOT_FOUND
    );
}

/// `/actuator` is not blanket-public: only `health` and `info` are listed.
#[tokio::test]
async fn the_rest_of_the_actuator_is_not_exposed() {
    let app = TestApp::spawn().await;
    for uri in ["/actuator", "/actuator/env", "/actuator/metrics"] {
        assert_eq!(
            get(&app, uri, None).await.status,
            StatusCode::UNAUTHORIZED,
            "GET {uri}"
        );
    }
}

/// A filter-level rejection carries no `instance`; a handler-level one does.
///
/// `instance` is written by `RequestResponseBodyMethodProcessor`, which is
/// Spring MVC and never sees a body the security filters wrote. Emitting one on
/// a 401 would be an added field, and an added field is as much a contract
/// break as a missing one.
#[tokio::test]
async fn only_handler_level_problems_name_the_request_path() {
    let app = TestApp::spawn().await;
    let non_admin = manager_id(&app, "SherlockMain").await.to_string();

    let filter_level = get(&app, "/api/me", None).await;
    assert_eq!(filter_level.status, StatusCode::UNAUTHORIZED);
    assert!(
        filter_level.json().get("instance").is_none(),
        "{}",
        filter_level.text()
    );
    filter_level.assert_no_json_nulls();

    let handler_level = get(&app, "/api/nope", Some(&non_admin)).await;
    assert_eq!(handler_level.status, StatusCode::NOT_FOUND);
    assert_eq!(handler_level.json()["instance"], "/api/nope");
}

/// Deviation (c) in PORTING.md: dev-profile `OPTIONS /api/**` used to 401,
/// because `DevSecurityConfig` never called `.cors()`. Here `CorsLayer` sits
/// outside `authorize` in both profiles, so a preflight is answered rather than
/// denied. Prod behaviour is unchanged while `FRONTEND_ORIGIN` is unset.
#[tokio::test]
async fn a_preflight_is_answered_rather_than_denied() {
    let app = TestApp::spawn().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/tournaments")
                .header("Origin", "http://localhost:5173")
                .header("Access-Control-Request-Method", "GET")
                .body(Body::empty())
                .expect("well-formed request"),
        )
        .await;
    assert_ne!(response.status, StatusCode::UNAUTHORIZED);
}

/// **Invariant: the authorization layer and the router must agree about what
/// a path *is*.** `authorize` matches its Ant patterns against
/// `req.uri().path()` -- the raw, undecoded path -- and axum's router
/// (matchit) also matches the raw path. Because the two agree, an encoded or
/// traversal spelling of an admin route matches *neither* the `/api/admin/**`
/// rule *nor* the admin route itself, and falls through to a 404.
///
/// That agreement is a property of matchit's behaviour, not of this codebase:
/// an upstream change to how axum normalises or decodes a path before routing
/// could silently decouple the two layers, which is exactly the shape of an
/// authorization bypass (a rule table that no longer covers what the router
/// will actually dispatch). Hence a pinned test rather than a comment.
///
/// A 404 is the *correct* answer here, not a weaker stand-in for 403: nothing
/// routes at these spellings, and the four-row table in `authorize.rs`'s
/// module doc (also PORTING.md §12's first "assumed wrong and corrected
/// against the running backend" bullet) says an authenticated caller under
/// `/api/**` is handed to the router, which is free to 404 its own way.
/// `%61` is `a`, so `/api/%61dmin/heroes` is the percent-encoded spelling of
/// `/api/admin/heroes`; `/api/x/../admin/heroes` is the `..`-traversal
/// spelling of the same route.
#[tokio::test]
async fn an_encoded_or_traversal_spelling_of_an_admin_route_reaches_neither_the_rule_nor_the_route()
{
    let app = TestApp::spawn().await;
    let admin = manager_id(&app, "NeonStrategist").await.to_string();
    let non_admin = manager_id(&app, "SherlockMain").await.to_string();

    // Baseline: the plain spelling is gated by the table as expected.
    assert_eq!(
        get(&app, "/api/admin/heroes", Some(&admin)).await.status,
        StatusCode::OK,
        "plain spelling, admin"
    );
    assert_eq!(
        get(&app, "/api/admin/heroes", Some(&non_admin))
            .await
            .status,
        StatusCode::FORBIDDEN,
        "plain spelling, non-admin"
    );

    // The encoded and traversal spellings: matched by neither `authorize`'s
    // raw-path patterns nor the router's own raw-path matching, so they 404
    // for any manager rather than answering 200 or 403.
    for uri in ["/api/%61dmin/heroes", "/api/x/../admin/heroes"] {
        assert_eq!(
            get(&app, uri, Some(&admin)).await.status,
            StatusCode::NOT_FOUND,
            "GET {uri} (admin)"
        );
        assert_eq!(
            get(&app, uri, Some(&non_admin)).await.status,
            StatusCode::NOT_FOUND,
            "GET {uri} (non-admin)"
        );
    }
}
