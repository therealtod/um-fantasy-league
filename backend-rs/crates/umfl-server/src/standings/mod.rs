//! The Live Standings screen: the leaderboard, the ticker, and the push that
//! tells a tab to refetch them.
//!
//! Oracle: `api/StandingsController.kt`, `standings/StandingsService.kt`,
//! `standings/StandingsQuery.kt`, `standings/StandingsSseHub.kt`.
//!
//! All three routes are public -- nobody needs an account to watch a
//! tournament, only to enter and draft one -- and all three open with
//! `require_tournament`, so an unknown id is a 404 rather than an empty board.
//!
//! There are no DTOs here. The board and the ticker are
//! [`umfl_domain::standings`]'s own wire types, which is where the fold lives
//! and where their `serde` attributes already are; wrapping them in a second
//! set of structs would be a copy to keep in step for nothing.

pub mod query;
pub mod service;
pub mod sse;

use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use umfl_domain::standings::{StandingsBoard, TickerEntry};

use crate::error::ApiResult;
use crate::http::extract::{AppPath, AppQuery};
use crate::state::AppState;

pub use sse::StandingsSseHub;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tournaments/{id}/standings", get(standings))
        .route("/api/tournaments/{id}/matches", get(matches))
        .route("/api/tournaments/{id}/standings/stream", get(stream))
}

/// The leaderboard, carrying its own column definitions -- the backend cannot
/// know which metrics exist until it has read `scoring_coefficient`.
async fn standings(
    State(state): State<AppState>,
    AppPath(id): AppPath<i64>,
) -> ApiResult<Json<StandingsBoard>> {
    crate::tournament::service::require_tournament(&state.pool, id).await?;
    Ok(Json(service::board(&state, id).await?))
}

/// `@RequestParam(required = false, defaultValue = "0") sinceMatchId` and
/// `@RequestParam(required = false, defaultValue = "25") limit`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TickerQuery {
    #[serde(default)]
    since_match_id: i64,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    25
}

/// Recorded matches, newest first -- the Standings ticker.
///
/// Pass the highest `matchId` already seen as `sinceMatchId` to poll for just
/// the new results. The id is the key, not `playedAt`: parallel tables in a
/// round share a timestamp, so `playedAt` is not unique.
async fn matches(
    State(state): State<AppState>,
    AppPath(id): AppPath<i64>,
    AppQuery(params): AppQuery<TickerQuery>,
) -> ApiResult<Json<Vec<TickerEntry>>> {
    crate::tournament::service::require_tournament(&state.pool, id).await?;
    // `coerceIn(1, 200)`: a clamp rather than a rejection, so an out-of-range
    // limit is served rather than 400'd, exactly as the Kotlin does.
    let limit = params.limit.clamp(1, 200) as usize;
    let ticker = service::ticker(&state, id, params.since_match_id, limit).await?;
    Ok(Json(ticker))
}

/// A bare "something changed" push -- no board or ticker payload duplicated
/// over the wire. The client already knows how to pull fresh data from the two
/// routes above; this only tells it when to.
async fn stream(State(state): State<AppState>, AppPath(id): AppPath<i64>) -> ApiResult<Response> {
    crate::tournament::service::require_tournament(&state.pool, id).await?;
    Ok(state.standings_hub.subscribe(id)?)
}
