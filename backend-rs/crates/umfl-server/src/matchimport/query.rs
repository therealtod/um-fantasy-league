//! The catalogue reads the importer resolves names against.
//!
//! Two deliberately narrow projections rather than calls into `hero::query` /
//! `map::query`: the importer wants *every* hero and board as `(id, name)`
//! pairs -- not the pool-scoped, priced hero view the roster screens read, and
//! not the map aggregate the admin screens write. Keeping them here also keeps
//! this feature off two other features' projection shapes, for the same
//! merge-contention reason DTOs live with their own feature module.
//!
//! Note the scope: the hero catalogue is **not** filtered to the tournament's
//! pool. `match_game_participants.hero_id` and `hero_bans.hero_id` reference
//! `heroes(id)` directly, so a hero outside the pool can still be recorded;
//! only the board pool is a real constraint, and that is checked separately.

use sqlx::PgExecutor;

/// Every hero, as `(id, name)` for [`umfl_domain::name_resolver::NameResolver`].
pub async fn hero_names(db: impl PgExecutor<'_>) -> sqlx::Result<Vec<(String, i64)>> {
    let rows = sqlx::query!("select id, name from heroes")
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().map(|r| (r.name, r.id)).collect())
}

/// Every board, same shape.
pub async fn map_names(db: impl PgExecutor<'_>) -> sqlx::Result<Vec<(String, i64)>> {
    let rows = sqlx::query!("select id, name from game_maps")
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().map(|r| (r.name, r.id)).collect())
}
