//! `/actuator/health` and `/actuator/info`.
//!
//! Oracle: `application.yml`'s `management.endpoints.web.exposure.include:
//! health,info` with `management.endpoint.health.show-details:
//! when-authorized`. An anonymous caller -- which is what both compose
//! healthchecks are -- therefore sees the status and **no `components` key**,
//! so that is what this returns. `deploy/docker-compose.prod.yml` probes it
//! with `wget -qO- http://localhost:8080/actuator/health`.
//!
//! `groups` is Boot's own doing rather than anything in `application.yml`:
//! `HealthEndpoint` always reports its registered group names, and Boot
//! registers `liveness` and `readiness` by default. Verified against the
//! running backend:
//!
//! ```text
//! GET /actuator/health -> {"groups":["liveness","readiness"],"status":"UP"}
//! ```
//!
//! The group *routes* (`/actuator/health/liveness`) are deliberately not
//! served: `exposure.include` is `health,info`, and the rule table denies
//! everything else under `/actuator` anyway.
//!
//! Known deviation: Spring Boot Actuator answered with
//! `application/vnd.spring-boot.actuator.v3+json`. This answers
//! `application/json`. Nothing reads the media type -- the healthchecks are a
//! `wget` and a `fetch(...).ok` -- and the vendor type is not part of the
//! frontend contract.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/actuator/health", get(health))
        .route("/actuator/info", get(info))
}

/// Boot's default group registry. Reported whatever the status: it is the set
/// of groups that *exist*, not a health result of its own.
///
/// The `UP` shape is verified against the running backend. The `DOWN` shape is
/// inferred from that structure rather than observed -- reaching it means
/// taking the database away, and Hikari blocks on connection acquisition well
/// past any probe's timeout instead of answering.
const HEALTH_GROUPS: [&str; 2] = ["liveness", "readiness"];

async fn health(State(state): State<AppState>) -> Response {
    // A plain `sqlx::query` rather than the macro: this must compile without
    // offline metadata, and `select 1` has nothing to check against a schema.
    match sqlx::query("select 1").execute(&state.pool).await {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({ "groups": HEALTH_GROUPS, "status": "UP" })),
        )
            .into_response(),
        Err(err) => {
            tracing::warn!(error = %err, "Health check could not reach the database");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "groups": HEALTH_GROUPS, "status": "DOWN" })),
            )
                .into_response()
        }
    }
}

/// Empty, as it is today: nothing populates `info.*` in any `application.yml`.
async fn info() -> Json<serde_json::Value> {
    Json(json!({}))
}
