//! A tournament's roster roles: what an admin defines, and what a manager
//! picks from for each hero on their roster.
//!
//! Only the definitions live here. The arithmetic -- which role was in force
//! for a round, what a game's stats are worth under it, and whether a roster's
//! assignments are legal -- is pure, in [`umfl_domain::roster_roles`]. A
//! manager's assignments belong to their entry, so they are written by
//! `crate::tournament`, alongside the slots.
//!
//! Points are never stored: a role's weights are mutable reference data
//! retuned with a bare UPDATE, and the role bonus is priced at read time by
//! the standings fold (AGENTS.md, "Nothing writes points").

pub mod admin_service;
pub mod query;
pub mod writer;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::auth::CurrentManager;
use crate::error::ApiResult;
use crate::http::extract::{AppPath, ValidJson};
use crate::state::AppState;

/// One role as it is stored: the definition an admin writes.
///
/// The domain's [`umfl_domain::roster_roles::RosterRole`] is the same thing
/// minus `tournament_id` and `sort_order`, which no rule reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub id: Option<i64>,
    pub tournament_id: i64,
    pub name: String,
    pub max_per_roster: Option<i32>,
    pub sort_order: i32,
    /// `(stat, coefficient)`, by stat name.
    pub weights: Vec<(String, Decimal)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDto {
    pub id: i64,
    pub tournament_id: i64,
    pub name: String,
    /// Absent when the role is uncapped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_per_roster: Option<i32>,
    pub sort_order: i32,
    pub weights: Vec<RoleWeightDto>,
}

/// `coefficient` keeps its `numeric(10,4)` scale on the wire, exactly as
/// `ScoringCoefficientDto`'s does.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleWeightDto {
    pub stat: String,
    #[serde(with = "crate::http::big_decimal")]
    pub coefficient: Decimal,
}

impl From<Role> for RoleDto {
    fn from(role: Role) -> Self {
        Self {
            id: role.id.expect("a saved role has an id"),
            tournament_id: role.tournament_id,
            name: role.name,
            max_per_roster: role.max_per_roster,
            sort_order: role.sort_order,
            weights: role
                .weights
                .into_iter()
                .map(|(stat, coefficient)| RoleWeightDto { stat, coefficient })
                .collect(),
        }
    }
}

/// `stat` must be present and non-blank; `coefficient` must be present.
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct RoleWeightRequest {
    #[garde(custom(required_text("stat is required")))]
    pub stat: Option<String>,
    #[garde(custom(required("coefficient is required")))]
    #[serde(default, with = "crate::http::big_decimal::option")]
    pub coefficient: Option<Decimal>,
}

/// Create and update share one shape: an update is a full replace, weights
/// included, exactly like a scoring rule set's.
///
/// An absent `weights` is an empty list -- a role that rewards nothing is
/// legal, if pointless, and is how an admin parks one while deciding.
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct RoleRequest {
    #[garde(custom(required_text("name is required")))]
    pub name: Option<String>,
    #[serde(default)]
    #[garde(custom(positive_if_present("maxPerRoster must be positive")))]
    pub max_per_roster: Option<i32>,
    #[serde(default)]
    #[garde(skip)]
    pub sort_order: i32,
    #[serde(default)]
    #[garde(dive)]
    pub weights: Vec<RoleWeightRequest>,
}

impl RoleRequest {
    /// Validation has already run before a handler calls this, so the
    /// `expect`s below are unreachable.
    fn to_fields(&self) -> admin_service::RoleFields {
        admin_service::RoleFields {
            name: self
                .name
                .as_deref()
                .expect("validated as present")
                .trim()
                .to_owned(),
            max_per_roster: self.max_per_roster,
            sort_order: self.sort_order,
            weights: self
                .weights
                .iter()
                .map(|w| {
                    (
                        w.stat.clone().expect("validated as present"),
                        w.coefficient.expect("validated as present"),
                    )
                })
                .collect(),
        }
    }
}

/// Fails on absent *and* on whitespace-only.
fn required_text(message: &'static str) -> impl Fn(&Option<String>, &()) -> garde::Result {
    move |value, _| match value {
        Some(text) if !text.trim().is_empty() => Ok(()),
        _ => Err(garde::Error::new(message)),
    }
}

/// Fails only on absent.
fn required<T>(message: &'static str) -> impl Fn(&Option<T>, &()) -> garde::Result {
    move |value, _| match value {
        Some(_) => Ok(()),
        None => Err(garde::Error::new(message)),
    }
}

/// Absent is uncapped; present has to be at least one.
fn positive_if_present(message: &'static str) -> impl Fn(&Option<i32>, &()) -> garde::Result {
    move |value, _| match value {
        Some(n) if *n <= 0 => Err(garde::Error::new(message)),
        _ => Ok(()),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tournaments/{tournament_id}/roles", get(list))
        .route(
            "/api/admin/tournaments/{tournament_id}/roles",
            post(admin_create),
        )
        .route(
            "/api/admin/tournaments/{tournament_id}/roles/{role_id}",
            put(admin_update).delete(admin_delete),
        )
}

/// Public, like the hero pool: a manager choosing a role needs to see what
/// each one rewards, and a visitor reading the standings may want to know why
/// a Role Bonus column says what it says.
///
/// Lists the roles whether or not the tournament currently uses them -- the
/// admin wizard edits them either way, and `TournamentDto.rolesEnabled` is
/// what the roster builder checks.
async fn list(
    State(state): State<AppState>,
    AppPath(tournament_id): AppPath<i64>,
) -> ApiResult<Json<Vec<RoleDto>>> {
    let roles = admin_service::list(&state, tournament_id).await?;
    Ok(Json(roles.into_iter().map(RoleDto::from).collect()))
}

// `Access::Admin` is enforced by `auth::authorize` for every `/api/admin/**`
// path. Each handler still takes `CurrentManager` for the identity, so who a
// route needs stays visible at the route.

async fn admin_create(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(tournament_id): AppPath<i64>,
    ValidJson(request): ValidJson<RoleRequest>,
) -> ApiResult<impl IntoResponse> {
    let role = admin_service::create(&state, tournament_id, request.to_fields()).await?;
    Ok((StatusCode::CREATED, Json(RoleDto::from(role))))
}

async fn admin_update(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, role_id)): AppPath<(i64, i64)>,
    ValidJson(request): ValidJson<RoleRequest>,
) -> ApiResult<Json<RoleDto>> {
    let role = admin_service::update(&state, tournament_id, role_id, request.to_fields()).await?;
    Ok(Json(RoleDto::from(role)))
}

async fn admin_delete(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, role_id)): AppPath<(i64, i64)>,
) -> ApiResult<StatusCode> {
    admin_service::delete(&state, tournament_id, role_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
