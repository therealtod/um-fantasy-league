//! Role reads.

use indexmap::IndexMap;
use rust_decimal::Decimal;
use sqlx::{PgConnection, PgExecutor};
use umfl_domain::roster_roles::{RoleBook, RosterRole};
use umfl_domain::tournament::Tournament;

use super::Role;

/// Every role of one tournament, weights included, in the order they are
/// offered: `sort_order`, then id -- insertion order -- so a tie is stable.
///
/// Two statements, so it takes the connection.
pub async fn find_by_tournament(
    conn: &mut PgConnection,
    tournament_id: i64,
) -> sqlx::Result<Vec<Role>> {
    let roots = sqlx::query!(
        "select id, tournament_id, name, max_per_roster, sort_order
         from roster_roles where tournament_id = $1 order by sort_order, id",
        tournament_id
    )
    .fetch_all(&mut *conn)
    .await?;

    let ids: Vec<i64> = roots.iter().map(|r| r.id).collect();
    let mut weights = weights_by_role(conn, &ids).await?;

    Ok(roots
        .into_iter()
        .map(|r| Role {
            id: Some(r.id),
            tournament_id: r.tournament_id,
            name: r.name,
            max_per_roster: r.max_per_roster,
            sort_order: r.sort_order,
            weights: weights.swap_remove(&r.id).unwrap_or_default(),
        })
        .collect())
}

/// Looks up a role by id alone. Filtering by tournament is the caller's job.
pub async fn find_by_id(conn: &mut PgConnection, role_id: i64) -> sqlx::Result<Option<Role>> {
    let Some(root) = sqlx::query!(
        "select id, tournament_id, name, max_per_roster, sort_order
         from roster_roles where id = $1",
        role_id
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    let mut weights = weights_by_role(conn, &[root.id]).await?;
    Ok(Some(Role {
        id: Some(root.id),
        tournament_id: root.tournament_id,
        name: root.name,
        max_per_roster: root.max_per_roster,
        sort_order: root.sort_order,
        weights: weights.swap_remove(&root.id).unwrap_or_default(),
    }))
}

/// The id of the role with this name in this tournament, if any -- the unique
/// `(tournament_id, name)` lookup create and update use to name a collision
/// before the index does.
pub async fn find_id_by_name(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    name: &str,
) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar!(
        "select id from roster_roles where tournament_id = $1 and name = $2",
        tournament_id,
        name
    )
    .fetch_optional(db)
    .await
}

/// How many entries have ever given a hero this role. Any at all and the
/// role cannot be deleted: its rows price somebody's past rounds.
pub async fn count_entries_assigning(db: impl PgExecutor<'_>, role_id: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        r#"select count(distinct entry_id) as "count!" from entry_hero_roles where role_id = $1"#,
        role_id
    )
    .fetch_one(db)
    .await
}

/// The roles every rule and the standings fold price against -- or `None`
/// when the tournament does not use roles.
///
/// This is the one place the `roles_enabled` toggle is read for a rule: every
/// caller passes the result straight on, and `None` is what "off" means to
/// [`umfl_domain::roster_roles`] and [`umfl_domain::standings`].
pub async fn book(
    conn: &mut PgConnection,
    tournament: &Tournament,
) -> sqlx::Result<Option<RoleBook>> {
    if !tournament.roles_enabled {
        return Ok(None);
    }
    let tournament_id = tournament.id.expect("a loaded tournament has an id");
    let roles = find_by_tournament(conn, tournament_id).await?;
    Ok(Some(RoleBook::new(roles.into_iter().map(|role| {
        RosterRole {
            id: role.id.expect("a loaded role has an id"),
            name: role.name,
            max_per_roster: role.max_per_roster,
            weights: role.weights.into_iter().collect(),
        }
    }))))
}

/// Several roles' weights at once, keyed by role, by stat name -- a role's
/// weights are a set, and alphabetical is a stable order to list one in.
async fn weights_by_role(
    conn: &mut PgConnection,
    role_ids: &[i64],
) -> sqlx::Result<IndexMap<i64, Vec<(String, Decimal)>>> {
    let mut by_role: IndexMap<i64, Vec<(String, Decimal)>> = IndexMap::new();
    if role_ids.is_empty() {
        return Ok(by_role);
    }
    let rows = sqlx::query!(
        "select role_id, stat, coefficient from roster_role_weights
         where role_id = any($1) order by role_id, stat",
        role_ids
    )
    .fetch_all(conn)
    .await?;
    for r in rows {
        by_role
            .entry(r.role_id)
            .or_default()
            .push((r.stat, r.coefficient));
    }
    Ok(by_role)
}
