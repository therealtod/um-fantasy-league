//! The one human this application models.
//!
//! A manager is the person who drafts a roster, as distinct from the competitor
//! who plays the real tournament (a free-text `match_participants.player_label`,
//! not an entity) and the `hero` they bring to it.
//!
//! There is no credit balance here. Budget is granted per registration
//! (`tournament_entries.credit_grant`), not held in a global wallet, so entering a
//! tournament costs nothing and cannot be blocked by a wallet running dry.

pub mod query;
pub mod writer;

use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::CurrentManager;
use crate::state::AppState;

/// A persisted `managers` row.
///
/// `id` is not an `Option`: the unsaved form is [`writer::insert`]'s argument
/// list, so every `Manager` value in the program is one that came out of the
/// database.
///
/// **Deliberately not `Serialize`.** This is the internal principal, not a
/// response body: `auth_user_id` is an `Option` with no
/// `skip_serializing_if`, so serialising it directly would emit
/// `"authUserId": null` and break `default-property-inclusion: non_null`. The
/// `/api/me` payload is a DTO that names the four fields it actually returns
/// (`id`, `handle`, `displayName`, `isAdmin`) and belongs with that feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manager {
    pub id: i64,
    pub handle: String,
    pub display_name: String,
    /// Supabase `auth.users.id` (the JWT `sub` claim). Null for dev-seeded
    /// managers with no linked identity.
    pub auth_user_id: Option<Uuid>,
    /// Grants the admin role on `/api/admin/**`. Our own data, independent of
    /// any identity provider.
    pub is_admin: bool,
}

/// The signed-in manager, as the app's top bar reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerDto {
    pub id: i64,
    pub handle: String,
    pub display_name: String,
    pub is_admin: bool,
}

impl From<Manager> for ManagerDto {
    fn from(manager: Manager) -> Self {
        Self {
            id: manager.id,
            handle: manager.handle,
            display_name: manager.display_name,
            is_admin: manager.is_admin,
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/me", get(me))
}

async fn me(CurrentManager(manager): CurrentManager) -> Json<ManagerDto> {
    Json(ManagerDto::from(manager))
}
