//! Board reads: the catalogue, and one tournament's pool.
//!
//! The split here is by direction: the catalogue's reads and the pool's reads
//! both land in this file, and their writes both land in `writer.rs` and
//! `pool_admin.rs`.

use sqlx::PgExecutor;

use super::GameMap;

/// Every board in the catalogue.
///
/// `order by id` rather than an unordered `select`, which happens to come
/// back in insertion order right up until the first `UPDATE` moves a row.
/// Ordering by id makes
/// that the *guarantee* rather than the observed behaviour, and it is the same
/// order -- ids are a `bigserial`.
pub async fn find_all(db: impl PgExecutor<'_>) -> sqlx::Result<Vec<GameMap>> {
    let rows = sqlx::query!("select id, name from game_maps order by id")
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| GameMap {
            id: Some(r.id),
            name: r.name,
        })
        .collect())
}

/// `findByName` -- `game_maps.name` is `unique`, so this is at most one row.
pub async fn find_by_name(db: impl PgExecutor<'_>, name: &str) -> sqlx::Result<Option<GameMap>> {
    let row = sqlx::query!("select id, name from game_maps where name = $1", name)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|r| GameMap {
        id: Some(r.id),
        name: r.name,
    }))
}

/// `findById`.
pub async fn find_by_id(db: impl PgExecutor<'_>, map_id: i64) -> sqlx::Result<Option<GameMap>> {
    let row = sqlx::query!("select id, name from game_maps where id = $1", map_id)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|r| GameMap {
        id: Some(r.id),
        name: r.name,
    }))
}

/// `findAllById` -- the batch add's lookup, which is also how it names the ids
/// that do not exist.
///
/// Ordered by id for the same reason [`find_all`] is: the caller echoes this
/// list straight back as the response body, and `findAllById`'s own order is
/// unspecified.
pub async fn find_all_by_id(
    db: impl PgExecutor<'_>,
    map_ids: &[i64],
) -> sqlx::Result<Vec<GameMap>> {
    if map_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query!(
        "select id, name from game_maps where id = any($1) order by id",
        map_ids
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| GameMap {
            id: Some(r.id),
            name: r.name,
        })
        .collect())
}

/// The set of boards this tournament may record a match on.
///
/// A `Vec`, not a set: the primary key on `tournament_maps` already makes the
/// rows distinct, and every caller either scans it or hands it to a policy.
pub async fn pool_map_ids(db: impl PgExecutor<'_>, tournament_id: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar!(
        "select map_id from tournament_maps where tournament_id = $1",
        tournament_id
    )
    .fetch_all(db)
    .await
}

/// The boards already in this tournament's pool, identity included -- what the
/// admin Map Pool wizard lists.
pub async fn pool_maps(db: impl PgExecutor<'_>, tournament_id: i64) -> sqlx::Result<Vec<GameMap>> {
    let rows = sqlx::query!(
        "select gm.id, gm.name
         from tournament_maps tm
         join game_maps gm on gm.id = tm.map_id
         where tm.tournament_id = $1
         order by gm.name",
        tournament_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| GameMap {
            id: Some(r.id),
            name: r.name,
        })
        .collect())
}

/// True when this tournament has a recorded game on this board.
///
/// Unlike a hero, a board *is* protected by the schema: `match_games` carries a
/// composite FK onto `(tournament_id, map_id)` in `tournament_maps`, so deleting
/// the pool row out from under a recorded game is a constraint violation. This
/// check runs first so the refusal is a 409 that names the board, rather than
/// the generic data-integrity backstop that names nothing.
pub async fn has_recorded_match(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    map_id: i64,
) -> sqlx::Result<bool> {
    sqlx::query_scalar!(
        r#"select exists(
             select 1 from match_games where tournament_id = $1 and map_id = $2
           ) as "exists!""#,
        tournament_id,
        map_id
    )
    .fetch_one(db)
    .await
}
