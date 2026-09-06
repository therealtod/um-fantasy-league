//! Create, update and delete tournaments.

use chrono::NaiveDate;
use umfl_domain::DomainError;
use umfl_domain::tournament::{Tournament, TournamentFormat, TournamentStatus};

use crate::error::ApiResult;
use crate::state::AppState;

use super::query;
use super::service::{purge_unlocked_entries, require_tournament};
use super::writer;

/// The fields an admin submits, once validation has run. `end_date` is the
/// one field that is genuinely optional, rather than merely "not yet
/// validated".
pub struct TournamentFields<'a> {
    pub name: &'a str,
    pub format: TournamentFormat,
    pub status: TournamentStatus,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub capacity: i32,
    pub roster_size: i32,
    pub credit_grant: i32,
    /// How many heroes a manager may exchange per swap window. Configuration,
    /// so it belongs on the form beside `roster_size` and `credit_grant`.
    ///
    /// `current_round` and `swap_window_open` deliberately do **not** appear
    /// here: they are operational state, moved only by [`advance_round`] and
    /// [`set_swap_window`]. An update is a full replace, so carrying them on
    /// this struct would let an admin renaming a tournament reset it to round
    /// one and shut an open window without meaning to.
    pub swaps_per_round: i32,
}

pub async fn create(state: &AppState, fields: TournamentFields<'_>) -> ApiResult<Tournament> {
    let mut tx = state.pool.begin().await?;
    if query::find_by_name(&mut *tx, fields.name).await?.is_some() {
        return Err(name_taken(fields.name).into());
    }

    let tournament = Tournament {
        id: None,
        name: fields.name.to_owned(),
        format: fields.format,
        status: fields.status,
        start_date: fields.start_date,
        end_date: fields.end_date,
        capacity: fields.capacity,
        roster_size: fields.roster_size,
        credit_grant: fields.credit_grant,
        // A new tournament starts in round one with the window shut. Both are
        // the column defaults too; stating them here keeps the returned value
        // equal to the row without a re-read.
        current_round: 1,
        swaps_per_round: fields.swaps_per_round,
        swap_window_open: false,
    };
    let id = writer::insert_tournament(&mut *tx, &tournament).await?;
    tx.commit().await?;
    Ok(Tournament {
        id: Some(id),
        ..tournament
    })
}

/// Full replace, including `status` — there is no status-transition state
/// machine: an admin is trusted to move a tournament through its lifecycle
/// sensibly.
///
/// Saving [`TournamentStatus::Live`] additionally purges every entry that
/// never locked a roster (see [`purge_unlocked_entries`]) — an unlocked entry
/// can never score once rosters are frozen, so leaving it registered would
/// only leave a dead zero row on the standings board. This runs whenever the
/// *saved* status is LIVE, not only the first time a tournament transitions
/// into it: an admin can reopen registration (moving back to
/// `RegistrationOpen`) and re-enter LIVE later, and any entry registered in
/// that window needs the same purge.
pub async fn update(
    state: &AppState,
    tournament_id: i64,
    fields: TournamentFields<'_>,
) -> ApiResult<Tournament> {
    let mut tx = state.pool.begin().await?;
    let existing = require_tournament(&mut *tx, tournament_id).await?;
    let collision = query::find_by_name(&mut *tx, fields.name).await?;
    if collision.is_some_and(|other| other.id != Some(tournament_id)) {
        return Err(name_taken(fields.name).into());
    }

    let tournament = Tournament {
        id: Some(tournament_id),
        name: fields.name.to_owned(),
        format: fields.format,
        status: fields.status,
        start_date: fields.start_date,
        end_date: fields.end_date,
        capacity: fields.capacity,
        roster_size: fields.roster_size,
        credit_grant: fields.credit_grant,
        // Carried over from the loaded row, not taken from the request. The
        // update is a full replace, so an admin correcting a typo in the name
        // would otherwise send this tournament back to round one and shut an
        // open swap window as a side effect. Only `advance_round` and
        // `set_swap_window` move these.
        current_round: existing.current_round,
        swaps_per_round: fields.swaps_per_round,
        swap_window_open: existing.swap_window_open,
    };
    writer::update_tournament(&mut *tx, &tournament).await?;

    if fields.status == TournamentStatus::Live {
        purge_unlocked_entries(&mut *tx, tournament_id).await?;
    }

    tx.commit().await?;
    Ok(tournament)
}

/// Move the tournament into its next round.
///
/// This is what opens the possibility of a swap: the allowance is a function of
/// `current_round`, so advancing is what hands every manager their next window
/// (see [`umfl_domain::roster_policy::swap_allowance`]). It deliberately does
/// **not** open the window itself -- that is [`set_swap_window`], a separate
/// decision, because the window has to be shut again before the round's
/// results are recorded or a manager could buy the heroes that just scored.
///
/// The increment happens in the database rather than by reading the value and
/// adding one, so two admins clicking at once cannot both write the same
/// round.
pub async fn advance_round(state: &AppState, tournament_id: i64) -> ApiResult<Tournament> {
    let mut tx = state.pool.begin().await?;
    let tournament = require_tournament(&mut *tx, tournament_id).await?;
    let current_round = writer::advance_current_round(&mut *tx, tournament_id).await?;
    tx.commit().await?;
    Ok(Tournament {
        current_round,
        ..tournament
    })
}

/// Open or shut the swap window.
pub async fn set_swap_window(
    state: &AppState,
    tournament_id: i64,
    open: bool,
) -> ApiResult<Tournament> {
    let mut tx = state.pool.begin().await?;
    let tournament = require_tournament(&mut *tx, tournament_id).await?;
    writer::set_swap_window_open(&mut *tx, tournament_id, open).await?;
    tx.commit().await?;
    Ok(Tournament {
        swap_window_open: open,
        ..tournament
    })
}

/// Delete a tournament and all its related data.
///
/// Every foreign key onto `tournaments` carries `on delete cascade`
/// (`V1__core_schema.sql`), so this operation automatically removes:
/// - `tournament_heroes` entries (hero pool with prices)
/// - `tournament_maps` entries (legal board pool)
/// - `tournament_entries` entries (manager registrations, and their slots)
/// - `scoring_rule_sets` entries and their coefficients
/// - `tournament_matches` entries and their participants/games/bans
///
/// Allowed for any tournament status, and requires only that the tournament
/// exists.
///
/// The [`crate::r#match::MatchResultCache`] drop is hygiene rather than
/// correctness: every standings route calls [`require_tournament`] first and
/// so 404s before a cached list could be served. It just stops a deleted
/// tournament's matches occupying the cache until something evicts them.
pub async fn delete(state: &AppState, tournament_id: i64) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    require_tournament(&mut *tx, tournament_id).await?;
    writer::delete_tournament(&mut *tx, tournament_id).await?;
    tx.commit().await?;
    state.match_cache.invalidate(tournament_id);
    Ok(())
}

fn name_taken(name: &str) -> DomainError {
    DomainError::conflict(format!("A tournament named '{name}' already exists."))
}
