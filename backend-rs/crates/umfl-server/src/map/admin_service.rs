//! Create and rename boards, and manage a tournament's board pool.
//!
//! Oracle: `map/AdminMapService.kt`. Every `@Transactional` method there is a
//! `pool.begin()` here and nothing else is (PORTING.md §7).
//!
//! There is no pure policy behind this one -- a board is a name, and the only
//! rules are a uniqueness check and the removal refusal below, both of which
//! are questions for the database rather than arithmetic over data in hand.

use sqlx::PgConnection;
use umfl_domain::DomainError;

use crate::error::ApiResult;
use crate::state::AppState;
use crate::tournament::service::require_tournament;

use super::{GameMap, pool_admin, query, writer};

pub async fn list(state: &AppState) -> ApiResult<Vec<GameMap>> {
    Ok(query::find_all(&state.pool).await?)
}

/// One tournament's pool.
///
/// **No `require_tournament`**, exactly as `AdminMapController.listPool` has
/// none: an unknown tournament reads as an empty pool rather than a 404. That
/// is a live behaviour of the endpoint, not an oversight to tidy up during a
/// port.
pub async fn list_pool(state: &AppState, tournament_id: i64) -> ApiResult<Vec<GameMap>> {
    Ok(query::pool_maps(&state.pool, tournament_id).await?)
}

pub async fn create(state: &AppState, name: &str) -> ApiResult<GameMap> {
    let mut tx = state.pool.begin().await?;
    if query::find_by_name(&mut *tx, name).await?.is_some() {
        return Err(name_taken(name).into());
    }
    let id = writer::insert(&mut *tx, name).await?;
    tx.commit().await?;
    Ok(GameMap {
        id: Some(id),
        name: name.to_owned(),
    })
}

/// Renames a board.
///
/// # The rename announcement is deferred
///
/// The Kotlin publishes `ReferenceDataRenamedEvent` here, because an assembled
/// `MatchResult` carries each game's `mapName` as a *copy* and
/// `MatchResultCache` therefore has to drop what it holds -- a rename is the
/// one staleness no match write announces.
///
/// There is no cache on this side yet: `match` is the next package on
/// PORTING.md §3b's list and owns both `MatchResultCache` and the invalidation
/// pair. Nothing is stale in the meantime, since nothing is cached. **Whoever
/// lands `match` hangs the invalidation off the `existing.name != name` test
/// below**, along with the identical one in `AdminHeroService.update`, and does
/// it as the two-phase pair the event bought: once inside the transaction, and
/// once after it ends -- committed *or* rolled back.
pub async fn update(state: &AppState, map_id: i64, name: &str) -> ApiResult<GameMap> {
    let mut tx = state.pool.begin().await?;
    let existing = require_map(&mut tx, map_id).await?;
    let collision = query::find_by_name(&mut *tx, name).await?;
    if collision.is_some_and(|other| other.id != Some(map_id)) {
        return Err(name_taken(name).into());
    }
    writer::update(&mut *tx, map_id, name).await?;

    // The exact condition the announcement is gated on: `image_url` and
    // `tournament_hero.cost` never reach an assembled match, so a rename is
    // the only edit here that can leave a cached copy spelling the old name.
    if existing.name != name {
        tracing::debug!(map_id, from = existing.name, to = name, "Board renamed");
    }
    tx.commit().await?;

    Ok(GameMap {
        id: Some(map_id),
        name: name.to_owned(),
    })
}

/// Adds one board to a tournament's pool. Idempotent -- there is nothing to
/// "re-price", because `tournament_map` has no column beyond its key.
pub async fn add_to_pool(state: &AppState, tournament_id: i64, map_id: i64) -> ApiResult<GameMap> {
    let mut tx = state.pool.begin().await?;
    require_tournament(&mut *tx, tournament_id).await?;
    let map = require_map(&mut tx, map_id).await?;
    pool_admin::add_to_pool(&mut *tx, tournament_id, map_id).await?;
    tx.commit().await?;
    Ok(map)
}

/// The batch counterpart to [`add_to_pool`], in one round trip.
///
/// An id that names no board fails the whole call rather than being skipped:
/// the admin picked from a list, so an id that is not there means the list they
/// picked from is stale, and silently adding the rest would hide that.
pub async fn add_batch_to_pool(
    state: &AppState,
    tournament_id: i64,
    map_ids: &[i64],
) -> ApiResult<Vec<GameMap>> {
    let mut tx = state.pool.begin().await?;
    require_tournament(&mut *tx, tournament_id).await?;

    let distinct = deduplicate(map_ids);
    let maps = query::find_all_by_id(&mut *tx, &distinct).await?;

    let found: Vec<i64> = maps.iter().filter_map(|m| m.id).collect();
    let mut missing: Vec<i64> = distinct
        .iter()
        .copied()
        .filter(|id| !found.contains(id))
        .collect();
    if !missing.is_empty() {
        missing.sort();
        let ids = missing
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(DomainError::not_found(format!("No map(s) with id(s) {ids}")).into());
    }

    pool_admin::add_to_pool_batch(&mut *tx, tournament_id, &distinct).await?;
    tx.commit().await?;
    Ok(maps)
}

/// Removes a board from a tournament's pool.
///
/// Refused as a 409 naming the board when the tournament already has a game
/// recorded on it -- see [`query::has_recorded_match`] for why the schema, and
/// not just policy, forbids it.
///
/// The check and the delete are two statements, so a game recorded in between
/// slips past the check and is stopped by the FK instead. That window is narrow
/// but real, so the violation is translated into the same conflict rather than
/// left to the data-integrity backstop, which would render it as the generic
/// "should never fire" 409. The FK is deferred to commit, which is past the
/// point where this function could still name the board -- hence
/// [`pool_admin::check_map_in_pool_now`], which pulls the check back inside.
pub async fn remove_from_pool(state: &AppState, tournament_id: i64, map_id: i64) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    require_tournament(&mut *tx, tournament_id).await?;
    require_map(&mut tx, map_id).await?;
    if query::has_recorded_match(&mut *tx, tournament_id, map_id).await? {
        return Err(recorded_match_conflict(tournament_id, map_id).into());
    }

    let removed = match delete_and_check(&mut tx, tournament_id, map_id).await {
        Ok(removed) => removed,
        Err(err) if is_foreign_key_violation(&err) => {
            return Err(recorded_match_conflict(tournament_id, map_id).into());
        }
        Err(err) => return Err(err.into()),
    };
    if !removed {
        return Err(DomainError::not_found(format!(
            "Map {map_id} is not in tournament {tournament_id}'s pool"
        ))
        .into());
    }
    tx.commit().await?;
    Ok(())
}

/// The delete and the FK re-check as one fallible unit, so the caller's `match`
/// covers both -- the Kotlin's `try { ... .also { ... } }`.
async fn delete_and_check(
    conn: &mut PgConnection,
    tournament_id: i64,
    map_id: i64,
) -> sqlx::Result<bool> {
    let removed = pool_admin::remove_from_pool(&mut *conn, tournament_id, map_id).await?;
    pool_admin::check_map_in_pool_now(conn).await?;
    Ok(removed)
}

/// `DataIntegrityViolationException`, narrowed to the one constraint that can
/// fire here.
fn is_foreign_key_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.is_foreign_key_violation())
}

async fn require_map(conn: &mut PgConnection, map_id: i64) -> ApiResult<GameMap> {
    query::find_by_id(&mut *conn, map_id)
        .await?
        .ok_or_else(|| DomainError::not_found(format!("No map with id {map_id}")).into())
}

fn name_taken(name: &str) -> DomainError {
    DomainError::conflict(format!("A map named '{name}' already exists."))
}

fn recorded_match_conflict(tournament_id: i64, map_id: i64) -> DomainError {
    DomainError::conflict(format!(
        "Map {map_id} has recorded matches in tournament {tournament_id} \
         and cannot be removed from its pool."
    ))
}

/// `mapIds.toSet()` -- a `LinkedHashSet`, so first-seen order survives.
fn deduplicate(map_ids: &[i64]) -> Vec<i64> {
    let mut seen = indexmap::IndexSet::with_capacity(map_ids.len());
    for id in map_ids {
        seen.insert(*id);
    }
    seen.into_iter().collect()
}
