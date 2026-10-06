//! Create, update, list and delete a tournament's roles.
//!
//! The rules are not here: [`umfl_domain::roster_roles::validate_weights`]
//! checks the *shape* of each weighted stat, and never the set -- which stats
//! a game records is whatever the tournament's stats sheet has columns for.

use rust_decimal::Decimal;
use sqlx::PgConnection;
use umfl_domain::roster_roles;
use umfl_domain::{DomainError, Violation};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::tournament::service::require_tournament;

use super::{Role, query, writer};

/// A role as the admin submitted it, once request validation has run.
#[derive(Debug, Clone)]
pub struct RoleFields {
    pub name: String,
    pub max_per_roster: Option<i32>,
    pub sort_order: i32,
    /// `(stat, coefficient)`, stats as submitted -- normalised by
    /// [`to_role`] once [`validate`] has checked the normalised form.
    pub weights: Vec<(String, Decimal)>,
}

/// Every role of `tournament_id`, in the order they are offered.
pub async fn list(state: &AppState, tournament_id: i64) -> ApiResult<Vec<Role>> {
    let mut conn = state.pool.acquire().await?;
    require_tournament(&mut *conn, tournament_id).await?;
    Ok(query::find_by_tournament(&mut conn, tournament_id).await?)
}

pub async fn create(state: &AppState, tournament_id: i64, fields: RoleFields) -> ApiResult<Role> {
    let mut tx = state.pool.begin().await?;
    require_tournament(&mut *tx, tournament_id).await?;
    validate(&fields)?;
    if query::find_id_by_name(&mut *tx, tournament_id, &fields.name)
        .await?
        .is_some()
    {
        return Err(name_taken(tournament_id, &fields.name).into());
    }

    let mut role = to_role(None, tournament_id, fields);
    role.id = Some(writer::insert_role(&mut tx, &role).await?);
    tx.commit().await?;
    Ok(sorted(role))
}

/// Full replace, weights included. Retuning a weight re-prices every round
/// the role has already scored -- the same as retuning a scoring coefficient,
/// and for the same reason: nothing stores a point.
pub async fn update(
    state: &AppState,
    tournament_id: i64,
    role_id: i64,
    fields: RoleFields,
) -> ApiResult<Role> {
    let mut tx = state.pool.begin().await?;
    require_role(&mut tx, tournament_id, role_id).await?;
    validate(&fields)?;
    let collision = query::find_id_by_name(&mut *tx, tournament_id, &fields.name).await?;
    if collision.is_some_and(|other| other != role_id) {
        return Err(name_taken(tournament_id, &fields.name).into());
    }

    let role = to_role(Some(role_id), tournament_id, fields);
    writer::update_role(&mut tx, &role).await?;
    tx.commit().await?;
    Ok(sorted(role))
}

/// Refused while any entry has assigned the role: those rows price somebody's
/// past rounds, and `entry_hero_roles.role_id` deliberately does not cascade.
/// Checked here rather than left to the foreign key, so the admin reads a
/// message that names the problem instead of the generic data-integrity 409.
pub async fn delete(state: &AppState, tournament_id: i64, role_id: i64) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let role = require_role(&mut tx, tournament_id, role_id).await?;
    let entries = query::count_entries_assigning(&mut *tx, role_id).await?;
    if entries > 0 {
        return Err(DomainError::conflict(format!(
            "{} is assigned on {entries} roster(s) and cannot be deleted. Rename or retune it \
             instead.",
            role.name
        ))
        .into());
    }
    writer::delete_role(&mut tx, role_id).await?;
    tx.commit().await?;
    Ok(())
}

async fn require_role(
    conn: &mut PgConnection,
    tournament_id: i64,
    role_id: i64,
) -> ApiResult<Role> {
    query::find_by_id(&mut *conn, role_id)
        .await?
        .filter(|role| role.tournament_id == tournament_id)
        .ok_or_else(|| {
            DomainError::not_found(format!("No role {role_id} for tournament {tournament_id}"))
                .into()
        })
}

/// Malformed and duplicate stats are caught here rather than left to the
/// primary key and the format CHECK, which would surface as the generic 409
/// with nothing naming the bad row. Rendered as a scoring-rule 422: a role's
/// weights are scoring configuration, the same as a rule set's coefficients.
fn validate(fields: &RoleFields) -> ApiResult<()> {
    let stats: Vec<String> = fields.weights.iter().map(|(s, _)| s.clone()).collect();
    let violations = roster_roles::validate_weights(&stats);
    if violations.is_empty() {
        return Ok(());
    }
    Err(ApiError::Domain(DomainError::ScoringRule(
        violations.into_iter().map(Violation::from).collect(),
    )))
}

fn to_role(id: Option<i64>, tournament_id: i64, fields: RoleFields) -> Role {
    Role {
        id,
        tournament_id,
        name: fields.name,
        max_per_roster: fields.max_per_roster,
        sort_order: fields.sort_order,
        weights: fields
            .weights
            .into_iter()
            .map(|(stat, coefficient)| (roster_roles::normalise_stat(&stat), coefficient))
            .collect(),
    }
}

/// The weights in the order a later read returns them, so a create or update
/// response matches the list endpoint.
fn sorted(mut role: Role) -> Role {
    role.weights.sort_by(|a, b| a.0.cmp(&b.0));
    role
}

fn name_taken(tournament_id: i64, name: &str) -> DomainError {
    DomainError::conflict(format!(
        "A role named '{name}' already exists for tournament {tournament_id}."
    ))
}
