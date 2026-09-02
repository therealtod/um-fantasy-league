//! `tournament_maps` writes -- the composite-keyed link table no aggregate maps.
//!
//! Oracle: the write half of `map/MapPoolAdminRepository.kt`.
//!
//! There is no non-key column here, so unlike the hero pool there is no
//! "re-price" to also cover: the only write is an idempotent add, its removal,
//! and the constraint re-check that removal needs.

use sqlx::PgExecutor;

/// Adds one board to a tournament's pool. Idempotent.
pub async fn add_to_pool(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    map_id: i64,
) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into tournament_maps (tournament_id, map_id)
         values ($1, $2) on conflict do nothing",
        tournament_id,
        map_id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// The same idempotent add, batched into one statement.
///
/// The Kotlin builds a `values (:tournamentId, :mapId0), ...` list by hand
/// because `JdbcClient` has no array binding; `unnest` does the same job with a
/// fixed statement, so nothing here grows with the batch size.
pub async fn add_to_pool_batch(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    map_ids: &[i64],
) -> sqlx::Result<()> {
    if map_ids.is_empty() {
        return Ok(());
    }
    sqlx::query!(
        "insert into tournament_maps (tournament_id, map_id)
         select $1, m from unnest($2::bigint[]) as t(m)
         on conflict do nothing",
        tournament_id,
        map_ids
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Removes a board from a tournament's pool. `false` when it was not there.
pub async fn remove_from_pool(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    map_id: i64,
) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        "delete from tournament_maps where tournament_id = $1 and map_id = $2",
        tournament_id,
        map_id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Fires `match_game_map_in_pool` **now**, in the caller's transaction, then
/// restores its deferral.
///
/// That FK is `DEFERRABLE INITIALLY DEFERRED` for the sake of tournament
/// deletes (see the schema comment on `match_games`), which means a pool row
/// deleted out from under a recorded game does not fail the `delete` -- it
/// fails at `COMMIT`, long after the service that could still name the board
/// and the tournament has returned. `set constraints ... immediate` pulls the
/// pending check back inside the caller, where it is translatable into a 409
/// that says which board.
///
/// The second statement never runs when the first fails, and does not need to:
/// the transaction is aborted at that point and the caller is about to roll it
/// back. That is the same shape as the Kotlin's `.also { }`.
pub async fn check_map_in_pool_now(conn: &mut sqlx::PgConnection) -> sqlx::Result<()> {
    sqlx::query("set constraints match_game_map_in_pool immediate")
        .execute(&mut *conn)
        .await?;
    sqlx::query("set constraints match_game_map_in_pool deferred")
        .execute(conn)
        .await?;
    Ok(())
}
