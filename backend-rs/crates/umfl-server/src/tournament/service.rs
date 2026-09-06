//! Registration, drafting and locking — the transaction boundaries.
//!
//! The rules themselves are not here: they are
//! [`umfl_domain::roster_policy`], called with their inputs passed in.

use chrono::Utc;
use indexmap::IndexMap;
use sqlx::{PgConnection, PgExecutor};
use umfl_domain::roster_policy::{self, BudgetStatus, RosterPick, RosterRule, RosterViolation};
use umfl_domain::tournament::{
    EntrySlot, EntryStatus, Tournament, TournamentEntry, TournamentStatus,
};
use umfl_domain::{DomainError, Violation};

use crate::error::{ApiError, ApiResult};
use crate::hero::{HeroView, query as hero_query};
use crate::manager::Manager;
use crate::state::AppState;

use super::{query, writer};

/// Postgres's default name for the inline `unique (tournament_id, manager_id)`
/// on `tournament_entries` (see `V1__core_schema.sql`).
const ENTRY_UNIQUE_INDEX: &str = "tournament_entry_tournament_id_manager_id_key";

/// An entry together with the heroes on it, priced now, and its budget position.
#[derive(Debug, Clone)]
pub struct RosterSnapshot {
    pub entry: TournamentEntry,
    pub tournament: Tournament,
    pub heroes: Vec<HeroView>,
    pub budget: BudgetStatus,
    /// Heroes this entry may still exchange, carry-over included. Derived from
    /// the swap log on every read, never stored.
    pub swaps_available: i32,
    /// Whether this entry has already spent its one submission for the
    /// tournament's current round.
    pub already_swapped_this_round: bool,
    /// Whether a swap would be accepted right now -- the swap counterpart to
    /// the `lockable` flag, and what the builder's submit button reads.
    pub swappable: bool,
}

pub async fn list_tournaments(
    db: impl PgExecutor<'_>,
    status: Option<TournamentStatus>,
) -> sqlx::Result<Vec<Tournament>> {
    match status {
        None => query::find_all_ordered(db).await,
        Some(status) => query::find_by_status_ordered(db, status).await,
    }
}

pub async fn enrolment_counts(db: impl PgExecutor<'_>) -> sqlx::Result<IndexMap<i64, i64>> {
    query::count_entries_per_tournament(db).await
}

pub async fn enrolment_count(db: impl PgExecutor<'_>, tournament_id: i64) -> sqlx::Result<i64> {
    query::count_entries(db, tournament_id).await
}

/// The 404 every tournament-scoped route opens with.
///
/// Public — `crate::hero`'s list handler calls it so an unknown tournament is a
/// 404 rather than a silently empty pool, exactly as `HeroController` does.
pub async fn require_tournament(db: impl PgExecutor<'_>, id: i64) -> ApiResult<Tournament> {
    query::find_by_id(db, id)
        .await?
        .ok_or_else(|| DomainError::not_found(format!("No tournament with id {id}")).into())
}

/// Enter a manager into a tournament and open an empty DRAFT roster.
///
/// Entering is free: there is no fee and no wallet. What registration does is
/// hand over a budget — the tournament's `credit_grant`, snapshotted onto the
/// entry so a later retune cannot disturb a roster drafted against it.
///
/// The two ways this can race are guarded differently. **Double registration**
/// is caught by `unique (tournament_id, manager_id)` even if the read below
/// misses a concurrent insert; the loser of that race has its insert translated
/// into the same "already registered" message the non-racing caller gets,
/// rather than the generic data-integrity 409, which names nothing.
/// **Capacity** has no such constraint — a count is not a row — so the last seat
/// is protected by locking the tournament row first: concurrent registrations
/// for the same tournament serialise there, and each counts entries only after
/// the previous has committed its own.
pub async fn register(
    state: &AppState,
    tournament_id: i64,
    manager: &Manager,
) -> ApiResult<RosterSnapshot> {
    let mut tx = state.pool.begin().await?;

    let tournament = require_tournament(&mut *tx, tournament_id).await?;
    if !tournament.accepts_registration() {
        return Err(DomainError::conflict(format!(
            "{} is {} and is not open for registration.",
            tournament.name, tournament.status
        ))
        .into());
    }
    if query::find_entry(&mut tx, tournament_id, manager.id)
        .await?
        .is_some()
    {
        return Err(already_registered(&tournament).into());
    }

    let capacity = query::lock_capacity_by_id(&mut *tx, tournament_id)
        .await?
        .ok_or_else(|| DomainError::not_found(format!("No tournament with id {tournament_id}")))?;
    if query::count_entries(&mut *tx, tournament_id).await? >= i64::from(capacity) {
        return Err(DomainError::conflict(format!(
            "{} is full ({capacity} entries).",
            tournament.name
        ))
        .into());
    }

    let mut entry = TournamentEntry {
        id: None,
        tournament_id,
        manager_id: manager.id,
        status: EntryStatus::Draft,
        credit_grant: tournament.credit_grant,
        registered_at: Utc::now(),
        locked_at: None,
        slots: Vec::new(),
    };
    entry.id = Some(
        writer::insert_entry(&mut tx, &entry)
            .await
            .map_err(|e| entry_conflict(e, &tournament))?,
    );

    let snapshot = snapshot(&mut tx, entry, tournament).await?;
    tx.commit().await?;
    Ok(snapshot)
}

/// The roster the caller already has here, or `None` if they have not entered.
///
/// Not transactional -- two reads, and nothing depends on their being one
/// snapshot.
pub async fn find_my_entry(
    state: &AppState,
    tournament_id: i64,
    manager: &Manager,
) -> ApiResult<Option<RosterSnapshot>> {
    let mut conn = state.pool.acquire().await?;
    let tournament = require_tournament(&mut *conn, tournament_id).await?;
    let Some(entry) = query::find_entry(&mut conn, tournament_id, manager.id).await? else {
        return Ok(None);
    };
    snapshot(&mut conn, entry, tournament).await.map(Some)
}

/// Replace the roster selection.
///
/// Over-budget selections are accepted while the entry is a DRAFT — the builder
/// is a scratchpad and shows a negative meter rather than blocking the edit —
/// and rejected at [`lock_roster`].
pub async fn set_slots(
    state: &AppState,
    tournament_id: i64,
    manager: &Manager,
    hero_ids: &[i64],
) -> ApiResult<RosterSnapshot> {
    let mut tx = state.pool.begin().await?;

    let tournament = require_tournament(&mut *tx, tournament_id).await?;
    let mut entry = require_my_entry(&mut tx, tournament_id, manager).await?;

    let picks = resolve_picks(&mut *tx, &tournament, hero_ids).await?;
    let violations = roster_policy::validate_draft(&picks, &tournament, entry.status);
    if !violations.is_empty() {
        return Err(roster_rule(violations));
    }

    // Only the hero is persisted: no cost snapshot, so an unlocked roster
    // re-prices when an admin retunes the pool.
    entry.slots = picks
        .iter()
        .map(|p| EntrySlot { hero_id: p.hero_id })
        .collect();
    writer::update_entry(&mut tx, &entry).await?;

    let snapshot = snapshot(&mut tx, entry, tournament).await?;
    tx.commit().await?;
    Ok(snapshot)
}

/// Commit the roster. Enforces roster size and the *entry's* credit grant.
pub async fn lock_roster(
    state: &AppState,
    tournament_id: i64,
    manager: &Manager,
) -> ApiResult<RosterSnapshot> {
    let mut tx = state.pool.begin().await?;

    let tournament = require_tournament(&mut *tx, tournament_id).await?;
    let mut entry = require_my_entry(&mut tx, tournament_id, manager).await?;

    let picks = resolve_picks(&mut *tx, &tournament, &entry.hero_ids()).await?;
    let violations = roster_policy::validate_lock(&picks, &tournament, &entry);
    if !violations.is_empty() {
        return Err(roster_rule(violations));
    }

    entry.lock(Utc::now());
    writer::update_entry(&mut tx, &entry).await?;

    let snapshot = snapshot(&mut tx, entry, tournament).await?;
    tx.commit().await?;
    Ok(snapshot)
}

/// Exchange heroes on a locked roster, during an open swap window.
///
/// Takes the **whole proposed roster** rather than a list of exchanges, and
/// derives the difference here. The builder already stages a full list, and
/// deriving the diff server-side is what stops the two sides disagreeing about
/// what counts as a swap -- a client that sent exchanges could describe a
/// three-hero shuffle as one move.
///
/// Writes the new slots and the log rows in one transaction: a roster that
/// changed without a log row would be a hero whose past points silently moved
/// with it, which is precisely what the log exists to prevent.
///
/// An unchanged roster is a no-op rather than an error -- it spends no
/// allowance and writes nothing, so a manager who opens the builder, changes
/// their mind and submits anyway keeps their window.
pub async fn swap_roster(
    state: &AppState,
    tournament_id: i64,
    manager: &Manager,
    hero_ids: &[i64],
) -> ApiResult<RosterSnapshot> {
    let mut tx = state.pool.begin().await?;

    let tournament = require_tournament(&mut *tx, tournament_id).await?;
    // Before anything is read: every rule below is decided against rows this
    // transaction goes on to write, and the one-submission-per-round rule has
    // no unique index behind it to catch a race the way double registration
    // has one. See `query::lock_entry_by_manager`.
    query::lock_entry_by_manager(&mut *tx, tournament_id, manager.id).await?;
    let mut entry = require_my_entry(&mut tx, tournament_id, manager).await?;
    let entry_id = entry.id.expect("a loaded entry has an id");

    let held = entry.hero_ids();
    let picks = resolve_picks(&mut *tx, &tournament, hero_ids).await?;

    let swaps_used = query::swap_count_for_entry(&mut *tx, entry_id).await?;
    let swaps_used = i32::try_from(swaps_used).unwrap_or(i32::MAX);
    let already_swapped =
        query::swapped_in_round(&mut *tx, entry_id, tournament.current_round).await?;

    let violations = roster_policy::validate_swap(
        &held,
        &picks,
        &tournament,
        &entry,
        swaps_used,
        already_swapped,
    );
    if !violations.is_empty() {
        return Err(roster_rule(violations));
    }

    // Pair departures with arrivals positionally. The pairing is bookkeeping,
    // not meaning: the policy has already established that both lists are the
    // same length, and nothing downstream reads a swap as "this *particular*
    // hero replaced that one" -- `holdings` only needs to know which hero left
    // and which arrived in a given round.
    let proposed: Vec<i64> = picks.iter().map(|p| p.hero_id).collect();
    let departing: Vec<i64> = held
        .iter()
        .copied()
        .filter(|id| !proposed.contains(id))
        .collect();
    let arriving: Vec<i64> = proposed
        .iter()
        .copied()
        .filter(|id| !held.contains(id))
        .collect();
    let exchanges: Vec<(i64, i64)> = departing.into_iter().zip(arriving).collect();

    entry.slots = picks
        .iter()
        .map(|p| EntrySlot { hero_id: p.hero_id })
        .collect();
    writer::update_entry(&mut tx, &entry).await?;
    writer::insert_roster_swaps(&mut tx, entry_id, tournament.current_round, &exchanges).await?;

    let snapshot = snapshot(&mut tx, entry, tournament).await?;
    tx.commit().await?;
    Ok(snapshot)
}

/// Drop every entry that never locked in a roster.
///
/// Called by `super::admin_service::update` whenever a tournament's status
/// is saved as [`TournamentStatus::Live`]. Once live,
/// `Tournament::accepts_roster_changes` is `false`, which makes
/// `roster_policy::validate_lock`'s `RosterRule::TournamentClosed` rule
/// reject locking forever after -- so an entry still in
/// [`EntryStatus::Draft`] at that point can never score a single point.
/// Leaving it registered would only leave a dead zero row on the standings
/// board, which is why this runs as part of the LIVE transition rather than
/// being left to an admin to notice. `crate::standings::query::rosters` also
/// only reads locked entries, as a second line of defense for the case this
/// purge is ever skipped or fails.
///
/// `on delete cascade` on `entry_slots` takes care of any picks the entry
/// held but never locked in.
///
/// Returns the number of entries purged.
pub async fn purge_unlocked_entries(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
) -> sqlx::Result<u64> {
    writer::delete_unlocked_entries(db, tournament_id).await
}

async fn require_my_entry(
    conn: &mut PgConnection,
    tournament_id: i64,
    manager: &Manager,
) -> ApiResult<TournamentEntry> {
    query::find_entry(conn, tournament_id, manager.id)
        .await?
        .ok_or_else(|| not_registered(tournament_id).into())
}

/// The 404 the controller raises for a manager with no entry here.
///
/// Lives beside [`require_my_entry`] so the two paths that can produce it —
/// `GET .../entries/me` and any write to a roster that does not exist — cannot
/// drift apart in wording.
pub fn not_registered(tournament_id: i64) -> DomainError {
    DomainError::not_found(format!(
        "You are not registered for tournament {tournament_id}."
    ))
}

/// Turn requested hero ids into priced picks, preserving the caller's ordering
/// so slot positions are stable.
///
/// The price comes from `tournament_heroes`, so "unknown" means *not in this
/// tournament's pool* — a stronger and more correct check than asking whether
/// the hero exists at all.
async fn resolve_picks(
    db: impl PgExecutor<'_>,
    tournament: &Tournament,
    hero_ids: &[i64],
) -> ApiResult<Vec<RosterPick>> {
    if hero_ids.is_empty() {
        return Ok(Vec::new());
    }
    let tournament_id = tournament.id.expect("a loaded tournament has an id");

    // `toSet()` on the way in: the query is asked for distinct ids, and
    // duplicates are restored below because `RosterPolicy` is what reports them.
    let mut distinct: Vec<i64> = Vec::new();
    for id in hero_ids {
        if !distinct.contains(id) {
            distinct.push(*id);
        }
    }
    let by_id: IndexMap<i64, HeroView> = hero_query::find_by_ids(db, tournament_id, &distinct)
        .await?
        .into_iter()
        .map(|hero| (hero.id, hero))
        .collect();

    let mut unknown: Vec<i64> = hero_ids
        .iter()
        .copied()
        .filter(|id| !by_id.contains_key(id))
        .collect();
    if !unknown.is_empty() {
        unknown.sort();
        unknown.dedup();
        let ids = unknown
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(roster_rule(vec![RosterViolation {
            rule: RosterRule::UnknownHero,
            message: format!("{} has no hero for id(s): {ids}.", tournament.name),
        }]));
    }

    // Duplicates survive this mapping on purpose — `validate_draft` reports them.
    Ok(hero_ids
        .iter()
        .map(|id| RosterPick {
            hero_id: *id,
            cost: by_id[id].cost,
        })
        .collect())
}

async fn snapshot(
    conn: &mut PgConnection,
    entry: TournamentEntry,
    tournament: Tournament,
) -> ApiResult<RosterSnapshot> {
    let tournament_id = tournament.id.expect("a loaded tournament has an id");
    let hero_ids = entry.hero_ids();

    // `find_roster_heroes`, not `find_by_ids`: a slot already committed to the
    // roster must still be reported (at cost 0) even if the hero has since left
    // this tournament's pool.
    let by_id: IndexMap<i64, HeroView> =
        hero_query::find_roster_heroes(&mut *conn, tournament_id, &hero_ids)
            .await?
            .into_iter()
            .map(|hero| (hero.id, hero))
            .collect();

    // Ordered by slot, not by the query's ordering.
    let heroes: Vec<HeroView> = hero_ids
        .iter()
        .filter_map(|id| by_id.get(id).cloned())
        .collect();

    let picks: Vec<RosterPick> = heroes
        .iter()
        .map(|h| RosterPick {
            hero_id: h.id,
            cost: h.cost,
        })
        .collect();
    let budget = roster_policy::budget_status(&picks, entry.credit_grant);

    // The swap state every roster response carries, so the builder can render
    // the window without a second round trip. Both numbers are counted off the
    // log rather than stored -- see `query::swap_count_for_entry`.
    let entry_id = entry.id.expect("a loaded entry has an id");
    let swaps_used = query::swap_count_for_entry(&mut *conn, entry_id).await?;
    let swaps_used = i32::try_from(swaps_used).unwrap_or(i32::MAX);
    let already_swapped_this_round =
        query::swapped_in_round(&mut *conn, entry_id, tournament.current_round).await?;
    let swaps_available = roster_policy::swap_allowance(
        tournament.swaps_per_round,
        tournament.current_round,
        swaps_used,
    );
    // `swappable` answers "would the button work", so it asks the policy the
    // same question the endpoint will -- with the roster unchanged, which
    // isolates the window/allowance rules from whatever the manager is about
    // to stage. `lockable` is computed the same way, in `mod.rs`.
    let swappable = roster_policy::validate_swap(
        &hero_ids,
        &picks,
        &tournament,
        &entry,
        swaps_used,
        already_swapped_this_round,
    )
    .is_empty()
        && swaps_available > 0;

    Ok(RosterSnapshot {
        swaps_available,
        already_swapped_this_round,
        swappable,
        entry,
        tournament,
        heroes,
        budget,
    })
}

fn roster_rule(violations: Vec<RosterViolation>) -> ApiError {
    DomainError::RosterRule(violations.into_iter().map(Violation::from).collect()).into()
}

fn already_registered(tournament: &Tournament) -> DomainError {
    DomainError::conflict(format!("Already registered for {}.", tournament.name))
}

/// The check in [`register`] and the insert are two statements, so a concurrent
/// registration can slip past the check and is stopped by
/// `unique (tournament_id, manager_id)` instead. That window is narrow but real,
/// so the violation is translated into the same conflict the non-racing caller
/// gets rather than left to the data-integrity backstop, which renders the
/// generic "should never fire" 409 and names nothing the manager can act on.
fn entry_conflict(err: sqlx::Error, tournament: &Tournament) -> ApiError {
    match &err {
        sqlx::Error::Database(db)
            if db.is_unique_violation() && db.constraint() == Some(ENTRY_UNIQUE_INDEX) =>
        {
            already_registered(tournament).into()
        }
        _ => ApiError::from_sqlx(err),
    }
}
