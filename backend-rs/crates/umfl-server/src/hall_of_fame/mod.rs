//! The Hall of Fame: every completed tournament's podium, and who has won the
//! most of them.
//!
//! Public, like the standings it is read from -- nobody needs an account to
//! see who won. There are no DTOs here: the wire types are
//! [`umfl_domain::hall_of_fame`]'s own, for the same reason the standings
//! module has none.

pub mod service;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use umfl_domain::hall_of_fame::HallOfFame;

use crate::error::ApiResult;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/hall-of-fame", get(hall_of_fame))
}

/// Completed tournaments newest first, each with its podium, plus the
/// all-time titles table.
async fn hall_of_fame(State(state): State<AppState>) -> ApiResult<Json<HallOfFame>> {
    Ok(Json(service::hall_of_fame(&state).await?))
}
