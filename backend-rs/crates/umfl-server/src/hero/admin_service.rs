//! Create and rename heroes, and manage a tournament's hero pool and
//! pricing.
//!
//! Oracle: `hero/AdminHeroService.kt`. Every `@Transactional` method there is
//! a `pool.begin()` here and nothing else is (PORTING.md §7).

use indexmap::{IndexMap, IndexSet};
use sqlx::PgConnection;
use umfl_domain::DomainError;

use crate::error::ApiResult;
use crate::state::AppState;
use crate::tournament::service::require_tournament;

use super::{Hero, HeroFilter, HeroPoolEntryInput, HeroView, pool_admin, query, writer};

pub async fn list(state: &AppState) -> ApiResult<Vec<Hero>> {
    Ok(query::find_all(&state.pool).await?)
}

/// A tournament's hero pool, priced -- see `AdminHeroController.listPool`'s
/// doc for why this is its own endpoint. Not transactional in the Kotlin
/// either: two reads, and nothing depends on their being one snapshot.
pub async fn pool(state: &AppState, tournament_id: i64) -> ApiResult<Vec<HeroView>> {
    require_tournament(&state.pool, tournament_id).await?;
    Ok(query::find_by_tournament(&state.pool, tournament_id, &HeroFilter::default()).await?)
}

pub async fn create(state: &AppState, name: &str, image_url: Option<&str>) -> ApiResult<Hero> {
    let mut tx = state.pool.begin().await?;
    if query::find_by_name(&mut *tx, name).await?.is_some() {
        return Err(name_taken(name).into());
    }
    let id = writer::insert(&mut *tx, name, image_url).await?;
    tx.commit().await?;
    Ok(Hero {
        id: Some(id),
        name: name.to_owned(),
        image_url: image_url.map(str::to_owned),
    })
}

/// A rename is announced; an image change is not.
///
/// `heroes.name` is copied into every assembled [`crate::r#match::MatchResult`]
/// that fielded, drafted or banned this hero, so a held match list still
/// spells the old name until something says otherwise -- see
/// [`crate::r#match::MatchResultCache`]. Nothing else on this row reaches a
/// match, which is why the invalidation is gated on the name actually
/// changing, exactly as `map::admin_service::update`'s identical rename
/// announcement is (see its doc for the two-phase reasoning: once inside the
/// transaction so a reader on this connection is not served the old name
/// back, and once after the transaction ends, committed *or* rolled back).
pub async fn update(
    state: &AppState,
    hero_id: i64,
    name: &str,
    image_url: Option<&str>,
) -> ApiResult<Hero> {
    let mut tx = state.pool.begin().await?;
    let existing = require_hero(&mut tx, hero_id).await?;
    let collision = query::find_by_name(&mut *tx, name).await?;
    if collision
        .as_ref()
        .is_some_and(|other| other.id != Some(hero_id))
    {
        return Err(name_taken(name).into());
    }
    writer::update(&mut *tx, hero_id, name, image_url).await?;

    let renamed = existing.name != name;
    if renamed {
        tracing::debug!(
            hero_id,
            from = existing.name,
            to = name,
            "Invalidating every cached match list: a hero was renamed."
        );
        state.match_cache.invalidate_all();
    }
    let outcome = tx.commit().await;
    if renamed {
        state.match_cache.invalidate_all();
    }
    outcome?;

    Ok(Hero {
        id: Some(hero_id),
        name: name.to_owned(),
        image_url: image_url.map(str::to_owned),
    })
}

/// Adds [`hero_id`] to [`tournament_id`]'s pool at [`cost`], or re-prices it
/// if already present.
pub async fn set_pool_cost(
    state: &AppState,
    tournament_id: i64,
    hero_id: i64,
    cost: i32,
) -> ApiResult<HeroView> {
    let mut tx = state.pool.begin().await?;
    require_tournament(&mut *tx, tournament_id).await?;
    require_hero(&mut tx, hero_id).await?;
    pool_admin::upsert_cost(&mut *tx, tournament_id, hero_id, cost).await?;
    let view = query::find_by_ids(&mut *tx, tournament_id, &[hero_id])
        .await?
        .into_iter()
        .next()
        .expect("just-priced hero not found");
    tx.commit().await?;
    Ok(view)
}

/// Adds/re-prices many heroes into [`tournament_id`]'s pool in one round
/// trip -- the batch counterpart to [`set_pool_cost`] for admin UIs that
/// stage several picks before submitting once. A `hero_id` repeated within
/// [`entries`] behaves like calling [`set_pool_cost`] for it twice: last
/// cost wins -- `IndexMap::insert` on an already-present key keeps the key's
/// first-seen position and replaces its value, exactly as Kotlin's
/// `entries.associateBy { it.heroId }.values.toList()` (a `LinkedHashMap`)
/// does.
pub async fn add_batch_to_pool(
    state: &AppState,
    tournament_id: i64,
    entries: &[HeroPoolEntryInput],
) -> ApiResult<Vec<HeroView>> {
    let mut tx = state.pool.begin().await?;
    require_tournament(&mut *tx, tournament_id).await?;

    let hero_ids: Vec<i64> = entries.iter().map(|e| e.hero_id).collect();
    require_heroes(&mut tx, &hero_ids).await?;

    let mut deduped: IndexMap<i64, i32> = IndexMap::new();
    for entry in entries {
        deduped.insert(entry.hero_id, entry.cost);
    }
    let deduped_entries: Vec<HeroPoolEntryInput> = deduped
        .iter()
        .map(|(&hero_id, &cost)| HeroPoolEntryInput { hero_id, cost })
        .collect();

    pool_admin::upsert_costs(&mut *tx, tournament_id, &deduped_entries).await?;
    let deduped_ids: Vec<i64> = deduped_entries.iter().map(|e| e.hero_id).collect();
    let views = query::find_by_ids(&mut *tx, tournament_id, &deduped_ids).await?;
    tx.commit().await?;
    Ok(views)
}

/// Removes [`hero_id`] from [`tournament_id`]'s pool. See
/// [`pool_admin::remove_from_pool`]'s doc for what this does to rosters that
/// already hold it.
pub async fn remove_from_pool(state: &AppState, tournament_id: i64, hero_id: i64) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    require_tournament(&mut *tx, tournament_id).await?;
    require_hero(&mut tx, hero_id).await?;
    if !pool_admin::remove_from_pool(&mut *tx, tournament_id, hero_id).await? {
        return Err(DomainError::not_found(format!(
            "Hero {hero_id} is not in tournament {tournament_id}'s pool"
        ))
        .into());
    }
    tx.commit().await?;
    Ok(())
}

async fn require_hero(conn: &mut PgConnection, hero_id: i64) -> ApiResult<Hero> {
    query::find_by_id(&mut *conn, hero_id)
        .await?
        .ok_or_else(|| DomainError::not_found(format!("No hero with id {hero_id}")).into())
}

/// `requireHeroes` -- a catalogue-wide existence check for a whole batch,
/// naming every id that resolves to nothing in one message.
async fn require_heroes(conn: &mut PgConnection, hero_ids: &[i64]) -> ApiResult<()> {
    let mut distinct = IndexSet::new();
    for id in hero_ids {
        distinct.insert(*id);
    }
    let distinct: Vec<i64> = distinct.into_iter().collect();
    let found = query::find_all_by_id(&mut *conn, &distinct).await?;
    let found_ids: IndexSet<i64> = found.into_iter().filter_map(|h| h.id).collect();

    let mut missing: Vec<i64> = distinct
        .into_iter()
        .filter(|id| !found_ids.contains(id))
        .collect();
    if !missing.is_empty() {
        missing.sort();
        let ids = missing
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(DomainError::not_found(format!("No hero(es) with id(s) {ids}")).into());
    }
    Ok(())
}

fn name_taken(name: &str) -> DomainError {
    DomainError::conflict(format!("A hero named '{name}' already exists."))
}
