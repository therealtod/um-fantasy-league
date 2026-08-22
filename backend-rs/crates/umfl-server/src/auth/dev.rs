//! `X-Manager-Id`: the dev/test credential.
//!
//! Oracle: `auth/DevManagerAuthenticationFilter.kt`.
//!
//! Resolving the header at this level rather than in a handler is the whole
//! point of the class this ports: without it the non-prod chain has no
//! principal at all, so an admin-only route could never be gated the same way
//! it is in prod. The role check and the handler's `CurrentManager` then read
//! the *same* resolved identity and can never disagree about who the request
//! is.
//!
//! Like its prod twin it knows nothing about which routes need an identity.

use axum::http::HeaderMap;

use crate::error::ApiError;
use crate::manager::{self, Manager};
use crate::state::AppState;

pub const MANAGER_ID_HEADER: &str = "X-Manager-Id";

/// `None` for an anonymous request, `Err` for a credential that is present and
/// unusable.
///
/// The three failures -- a header that is not a number, a number naming no
/// manager, and a lookup that could not run -- are all one 401 on the wire, as
/// they are in the Kotlin: the body is
/// `ProblemDetailAuthenticationEntryPoint`'s fixed sentence whatever the cause,
/// and the cause is logged rather than handed to the client.
pub async fn resolve(state: &AppState, headers: &HeaderMap) -> Result<Option<Manager>, ApiError> {
    let Some(header) = headers.get(MANAGER_ID_HEADER) else {
        return Ok(None);
    };

    let Some(manager_id) = header.to_str().ok().and_then(|v| v.parse::<i64>().ok()) else {
        tracing::warn!(
            "Rejected a malformed {MANAGER_ID_HEADER}: {:?}",
            header.to_str().unwrap_or("<not utf-8>")
        );
        return Err(ApiError::Unauthorized);
    };

    match manager::query::find_by_id(&state.pool, manager_id).await {
        Ok(Some(manager)) => Ok(Some(manager)),
        Ok(None) => {
            tracing::warn!("Rejected {MANAGER_ID_HEADER}: no manager with id {manager_id}");
            Err(ApiError::Unauthorized)
        }
        Err(err) => {
            tracing::error!(error = %err, "Could not resolve {MANAGER_ID_HEADER}");
            Err(ApiError::Unauthorized)
        }
    }
}
