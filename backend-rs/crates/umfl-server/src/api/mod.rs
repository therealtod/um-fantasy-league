//! Route assembly.
//!
//! Each feature exposes `pub fn routes() -> Router<AppState>` from its own
//! module and is merged here in one line. That is deliberately the *only*
//! place independent feature tasks touch in common -- DTOs live with their
//! feature rather than in a shared `Dtos.kt`, precisely so this stays the
//! single merge point.

pub mod actuator;

use axum::Router;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(actuator::routes())
        .merge(crate::manager::routes())
        .merge(crate::hero::routes())
        .merge(crate::tournament::routes())
        .merge(crate::scoring::routes())
        .merge(crate::map::routes())
        .merge(crate::r#match::routes())
    // Feature routes are merged here, one line each:
    //   .merge(crate::standings::routes())
    //   ...
}
