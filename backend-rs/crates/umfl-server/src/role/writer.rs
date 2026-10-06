//! Role writes.
//!
//! A role's weights are replaced wholesale on every update rather than
//! diffed, for the same reason a scoring rule set's coefficients are: an
//! in-place child update would have to reconcile the `(role_id, stat)` key
//! mid-statement, where a delete-and-reinsert never meets it.
//!
//! Every function takes the connection: each is more than one statement, and
//! all of them belong to somebody's transaction.

use rust_decimal::Decimal;
use sqlx::PgConnection;

use super::Role;

/// Inserts a role and its weights, returning the generated id.
pub async fn insert_role(conn: &mut PgConnection, role: &Role) -> sqlx::Result<i64> {
    let id = sqlx::query_scalar!(
        "insert into roster_roles (tournament_id, name, max_per_roster, sort_order)
         values ($1, $2, $3, $4) returning id",
        role.tournament_id,
        role.name,
        role.max_per_roster,
        role.sort_order
    )
    .fetch_one(&mut *conn)
    .await?;
    insert_weights(conn, id, &role.weights).await?;
    Ok(id)
}

/// Updates an existing role: the root row, then its weights wholesale.
///
/// # Panics
///
/// On a role with no id. Unreachable -- the only caller loaded it from
/// [`super::query::find_by_id`].
pub async fn update_role(conn: &mut PgConnection, role: &Role) -> sqlx::Result<()> {
    let id = role.id.expect("a loaded role has an id");
    sqlx::query!(
        "update roster_roles set name = $2, max_per_roster = $3, sort_order = $4 where id = $1",
        id,
        role.name,
        role.max_per_roster,
        role.sort_order
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!("delete from roster_role_weights where role_id = $1", id)
        .execute(&mut *conn)
        .await?;
    insert_weights(conn, id, &role.weights).await
}

/// Deletes a role. Its weights cascade; an assignment naming it does not,
/// which is why [`super::admin_service::delete`] checks for one first.
pub async fn delete_role(conn: &mut PgConnection, role_id: i64) -> sqlx::Result<()> {
    sqlx::query!("delete from roster_roles where id = $1", role_id)
        .execute(conn)
        .await?;
    Ok(())
}

async fn insert_weights(
    conn: &mut PgConnection,
    role_id: i64,
    weights: &[(String, Decimal)],
) -> sqlx::Result<()> {
    if weights.is_empty() {
        return Ok(());
    }
    let stats: Vec<String> = weights.iter().map(|(stat, _)| stat.clone()).collect();
    let coefficients: Vec<Decimal> = weights.iter().map(|(_, c)| *c).collect();
    sqlx::query!(
        "insert into roster_role_weights (role_id, stat, coefficient)
         select $1, stat, coefficient from unnest($2::text[], $3::numeric[]) as w (stat, coefficient)",
        role_id,
        &stats,
        &coefficients
    )
    .execute(conn)
    .await?;
    Ok(())
}
