//! Board catalogue writes.
//!
//! `game_maps` owns no child collection, so there is none of the
//! delete-and-reinsert cascade `scoring::writer` and `tournament::writer` have
//! to reproduce -- a board is two columns and one of them is the key.

use sqlx::PgExecutor;

/// Inserts a board, returning the generated id.
pub async fn insert(db: impl PgExecutor<'_>, name: &str) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        "insert into game_maps (name) values ($1) returning id",
        name
    )
    .fetch_one(db)
    .await
}

/// Renames an existing board.
pub async fn update(db: impl PgExecutor<'_>, map_id: i64, name: &str) -> sqlx::Result<()> {
    sqlx::query!("update game_maps set name = $1 where id = $2", name, map_id)
        .execute(db)
        .await?;
    Ok(())
}
