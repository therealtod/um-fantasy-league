//! `tournament_heroes` writes -- the composite-keyed link table no aggregate
//! maps.
//!
//! Oracle: `hero/HeroPoolAdminRepository.kt`.
//!
//! There is no separate "add to pool" and "re-price": the row's only non-key
//! column is `cost`, so an upsert covers both -- a hero not yet in the pool
//! gets added at the given cost, one already there gets re-priced.

use sqlx::PgExecutor;

use super::HeroPoolEntryInput;

pub async fn upsert_cost(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    hero_id: i64,
    cost: i32,
) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into tournament_heroes (tournament_id, hero_id, cost)
         values ($1, $2, $3)
         on conflict (tournament_id, hero_id) do update set cost = excluded.cost",
        tournament_id,
        hero_id,
        cost
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Same upsert as [`upsert_cost`], batched into a single statement via
/// `unnest` so N additions cost one round trip instead of N -- the Kotlin
/// builds a `values (:tournamentId, :heroId0, :cost0), ...` list by hand
/// because `JdbcClient` has no array binding; `unnest` does the same job
/// with a fixed statement.
pub async fn upsert_costs(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    entries: &[HeroPoolEntryInput],
) -> sqlx::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let hero_ids: Vec<i64> = entries.iter().map(|e| e.hero_id).collect();
    let costs: Vec<i32> = entries.iter().map(|e| e.cost).collect();
    sqlx::query!(
        "insert into tournament_heroes (tournament_id, hero_id, cost)
         select $1, h, c from unnest($2::bigint[], $3::integer[]) as t(h, c)
         on conflict (tournament_id, hero_id) do update set cost = excluded.cost",
        tournament_id,
        &hero_ids,
        &costs
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Removes [`hero_id`] from [`tournament_id`]'s pool. Returns false if it
/// wasn't there to begin with.
///
/// Nothing in the schema stops this -- `entry_slots.hero_id` references
/// `heroes(id)` directly, never `tournament_heroes` -- so a roster still
/// holding this hero doesn't break. It re-prices to 0 on its next read via
/// the `coalesce` in [`super::query::find_roster_heroes`], the same
/// "cost is never snapshotted" behaviour that already applies when a hero is
/// merely re-priced rather than removed. That is deliberate, not an
/// oversight: see `RosterPolicy`'s "no cost snapshot" invariant.
pub async fn remove_from_pool(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    hero_id: i64,
) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        "delete from tournament_heroes where tournament_id = $1 and hero_id = $2",
        tournament_id,
        hero_id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}
