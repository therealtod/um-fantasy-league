//! Tournament and entry reads. All `query!` reads live in the file the naming
//! convention reserves for reads; the *write* half of an entry aggregate is
//! `writer.rs`.

use indexmap::IndexMap;
use sqlx::PgExecutor;
use umfl_domain::standings::RosterSwap;
use umfl_domain::tournament::{
    EntrySlot, EntryStatus, Tournament, TournamentEntry, TournamentFormat, TournamentStatus,
};

/// Every tournament, ordered by start date.
///
/// Ties on `start_date` are left to the database rather than broken with an
/// explicit id tiebreak.
pub async fn find_all_ordered(db: impl PgExecutor<'_>) -> sqlx::Result<Vec<Tournament>> {
    let rows = sqlx::query!(
        r#"select id, name, format, status, start_date, end_date,
                  capacity, roster_size, credit_grant,
                  current_round, swaps_per_round, swap_window_open
           from tournaments order by start_date asc"#
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(Tournament {
                id: Some(r.id),
                name: r.name,
                format: format_from_db(&r.format)?,
                status: status_from_db(&r.status)?,
                start_date: r.start_date,
                end_date: r.end_date,
                capacity: r.capacity,
                roster_size: r.roster_size,
                credit_grant: r.credit_grant,
                current_round: r.current_round,
                swaps_per_round: r.swaps_per_round,
                swap_window_open: r.swap_window_open,
            })
        })
        .collect()
}

/// `findByStatusOrderByStartDateAsc`.
pub async fn find_by_status_ordered(
    db: impl PgExecutor<'_>,
    status: TournamentStatus,
) -> sqlx::Result<Vec<Tournament>> {
    let rows = sqlx::query!(
        r#"select id, name, format, status, start_date, end_date,
                  capacity, roster_size, credit_grant,
                  current_round, swaps_per_round, swap_window_open
           from tournaments where status = $1 order by start_date asc"#,
        status.as_str()
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(Tournament {
                id: Some(r.id),
                name: r.name,
                format: format_from_db(&r.format)?,
                status: status_from_db(&r.status)?,
                start_date: r.start_date,
                end_date: r.end_date,
                capacity: r.capacity,
                roster_size: r.roster_size,
                credit_grant: r.credit_grant,
                current_round: r.current_round,
                swaps_per_round: r.swaps_per_round,
                swap_window_open: r.swap_window_open,
            })
        })
        .collect()
}

pub async fn find_by_id(db: impl PgExecutor<'_>, id: i64) -> sqlx::Result<Option<Tournament>> {
    let Some(r) = sqlx::query!(
        r#"select id, name, format, status, start_date, end_date,
                  capacity, roster_size, credit_grant,
                  current_round, swaps_per_round, swap_window_open
           from tournaments where id = $1"#,
        id
    )
    .fetch_optional(db)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(Tournament {
        id: Some(r.id),
        name: r.name,
        format: format_from_db(&r.format)?,
        status: status_from_db(&r.status)?,
        start_date: r.start_date,
        end_date: r.end_date,
        capacity: r.capacity,
        roster_size: r.roster_size,
        credit_grant: r.credit_grant,
        current_round: r.current_round,
        swaps_per_round: r.swaps_per_round,
        swap_window_open: r.swap_window_open,
    }))
}

/// `findByName` -- `tournaments.name` is `unique`, so this is at most one row.
/// The admin create/update collision check's oracle.
pub async fn find_by_name(db: impl PgExecutor<'_>, name: &str) -> sqlx::Result<Option<Tournament>> {
    let Some(r) = sqlx::query!(
        r#"select id, name, format, status, start_date, end_date,
                  capacity, roster_size, credit_grant,
                  current_round, swaps_per_round, swap_window_open
           from tournaments where name = $1"#,
        name
    )
    .fetch_optional(db)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(Tournament {
        id: Some(r.id),
        name: r.name,
        format: format_from_db(&r.format)?,
        status: status_from_db(&r.status)?,
        start_date: r.start_date,
        end_date: r.end_date,
        capacity: r.capacity,
        roster_size: r.roster_size,
        credit_grant: r.credit_grant,
        current_round: r.current_round,
        swaps_per_round: r.swaps_per_round,
        swap_window_open: r.swap_window_open,
    }))
}

/// Take a row lock on the tournament and return its **current** capacity.
///
/// `capacity` has no database constraint behind it the way double registration
/// has `unique (tournament_id, manager_id)`, so the count-then-insert in
/// [`super::service::register`] needs the seat check and the insert serialised
/// against each other. Locking the tournament row — not the entries — gives
/// every concurrent registration for the same tournament one queue to stand in,
/// and costs nothing anywhere else: no other statement on the manager path
/// writes `tournaments`.
///
/// It returns capacity rather than just the id because the lock alone only
/// serialises the *write*: a concurrent admin capacity change commits and
/// releases before this transaction's `FOR UPDATE` is granted, so the
/// `Tournament` the caller already holds carries the pre-update value.
/// Re-reading here, after the lock, is what makes the seat check exact.
///
/// **Must be called on the transaction**, not the pool: a lock taken on a
/// pooled connection that is then returned is a lock released immediately.
pub async fn lock_capacity_by_id(db: impl PgExecutor<'_>, id: i64) -> sqlx::Result<Option<i32>> {
    sqlx::query_scalar!(
        "select capacity from tournaments where id = $1 for update",
        id
    )
    .fetch_optional(db)
    .await
}

/// Take a row lock on one manager's entry, returning its id.
///
/// [`super::service::swap_roster`] reads the swap log, decides against it and
/// then writes to it. Unlike double registration — which
/// `unique (tournament_id, manager_id)` refuses outright — the
/// one-submission-per-round rule has no index standing behind it and cannot
/// have one: a submission legitimately writes several rows sharing a round, so
/// `(entry_id, round)` is not unique. Two requests for the same entry — a
/// double-clicked submit is enough — would otherwise both read
/// `swapped_in_round` as false and both write. That spends the allowance twice
/// for one exchange and, worse, leaves a log that no longer replays to the
/// roster: `EntryRoster::holdings` rewinds the duplicate pair into a phantom
/// second holding, and the arriving hero scores twice on the board.
///
/// Locking the entry row gives every submission for that entry one queue to
/// stand in, and costs nothing anywhere else: nothing on the standings or admin
/// path writes a `tournament_entries` row a manager could be swapping on.
///
/// `None` when the caller has no entry here — [`super::service::swap_roster`]
/// reports that from the load that follows.
///
/// **Must be called on the transaction**, not the pool: a lock taken on a
/// pooled connection that is then returned is a lock released immediately.
pub async fn lock_entry_by_manager(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    manager_id: i64,
) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar!(
        "select id from tournament_entries
          where tournament_id = $1 and manager_id = $2
          for update",
        tournament_id,
        manager_id
    )
    .fetch_optional(db)
    .await
}

/// `countByTournamentId`.
pub async fn count_entries(db: impl PgExecutor<'_>, tournament_id: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        r#"select count(*) as "count!" from tournament_entries where tournament_id = $1"#,
        tournament_id
    )
    .fetch_one(db)
    .await
}

/// Enrolment counts for every tournament in one round trip, so the Lobby does
/// not issue a count query per card.
pub async fn count_entries_per_tournament(
    db: impl PgExecutor<'_>,
) -> sqlx::Result<IndexMap<i64, i64>> {
    let rows = sqlx::query!(
        r#"select t.id as tournament_id, count(e.id) as "entry_count!"
           from tournaments t
           left join tournament_entries e on e.tournament_id = t.id
           group by t.id"#
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.tournament_id, r.entry_count))
        .collect())
}

/// The caller's entry for one tournament, loading the aggregate whole.
///
/// `slots` is populated with a second query keyed on `entry_id`, ordered by
/// `slot_index` because the list position **is** the slot index. See the
/// `EntrySlot` doc in
/// `umfl-domain`: nothing may reorder this `Vec`.
///
/// The one read here that takes a connection rather than an executor, because
/// it is two statements and an `impl PgExecutor` is consumed by the first.
/// Callers outside a transaction acquire one from the pool; callers inside pass
/// `&mut *tx`, so the aggregate is loaded under the same snapshot as the write
/// that follows it.
pub async fn find_entry(
    conn: &mut sqlx::PgConnection,
    tournament_id: i64,
    manager_id: i64,
) -> sqlx::Result<Option<TournamentEntry>> {
    let Some(r) = sqlx::query!(
        r#"select id, tournament_id, manager_id, status, credit_grant, registered_at, locked_at
           from tournament_entries where tournament_id = $1 and manager_id = $2"#,
        tournament_id,
        manager_id
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };

    let slots = sqlx::query_scalar!(
        "select hero_id from entry_slots where entry_id = $1 order by slot_index",
        r.id
    )
    .fetch_all(&mut *conn)
    .await?;

    Ok(Some(TournamentEntry {
        id: Some(r.id),
        tournament_id: r.tournament_id,
        manager_id: r.manager_id,
        status: entry_status_from_db(&r.status)?,
        credit_grant: r.credit_grant,
        registered_at: r.registered_at,
        locked_at: r.locked_at,
        slots: slots
            .into_iter()
            .map(|hero_id| EntrySlot { hero_id })
            .collect(),
    }))
}

/// Entry status per tournament for one manager, backed by
/// `idx_tournament_entry_manager`.
///
/// A projection rather than a loaded aggregate: the Lobby only asks "which of
/// these am I in, and is my roster locked?", and loading entries to answer that
/// costs a second query per entry to populate `entry_slots`.
pub async fn statuses_by_tournament(
    db: impl PgExecutor<'_>,
    manager_id: i64,
) -> sqlx::Result<IndexMap<i64, EntryStatus>> {
    let rows = sqlx::query!(
        "select tournament_id, status from tournament_entries where manager_id = $1",
        manager_id
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|r| Ok((r.tournament_id, entry_status_from_db(&r.status)?)))
        .collect()
}

/// Entry status for one manager in one tournament, backed by the same index.
pub async fn status_for(
    db: impl PgExecutor<'_>,
    manager_id: i64,
    tournament_id: i64,
) -> sqlx::Result<Option<EntryStatus>> {
    let row = sqlx::query_scalar!(
        "select status from tournament_entries where manager_id = $1 and tournament_id = $2",
        manager_id,
        tournament_id
    )
    .fetch_optional(db)
    .await?;
    row.map(|s| entry_status_from_db(&s)).transpose()
}

// The three text columns below carry CHECK constraints naming exactly these
// values, so an unknown one means the schema moved underneath the code. That
// is a decode failure rather than a panic: `From<sqlx::Error>` renders it as
// a 500 naming which column.

/// The constant name the column stores. The admin writer's counterpart to
/// [`format_from_db`].
pub(crate) fn format_to_db(format: TournamentFormat) -> &'static str {
    match format {
        TournamentFormat::Banquest => "BANQUEST",
        TournamentFormat::Arsenal => "ARSENAL",
    }
}

/// The admin writer's counterpart to [`status_from_db`]. `TournamentStatus`
/// already carries `as_str` for the same purpose, since its rendering is
/// user-visible wire text (`TOURNAMENT_CLOSED`'s message); this is just that
/// method under the name its sibling encoders here use.
pub(crate) fn status_to_db(status: TournamentStatus) -> &'static str {
    status.as_str()
}

fn format_from_db(raw: &str) -> sqlx::Result<TournamentFormat> {
    match raw {
        "BANQUEST" => Ok(TournamentFormat::Banquest),
        "ARSENAL" => Ok(TournamentFormat::Arsenal),
        other => Err(decode(format!("unknown tournaments.format {other:?}"))),
    }
}

fn status_from_db(raw: &str) -> sqlx::Result<TournamentStatus> {
    match raw {
        "SCHEDULED" => Ok(TournamentStatus::Scheduled),
        "REGISTRATION_OPEN" => Ok(TournamentStatus::RegistrationOpen),
        "LIVE" => Ok(TournamentStatus::Live),
        "COMPLETED" => Ok(TournamentStatus::Completed),
        other => Err(decode(format!("unknown tournaments.status {other:?}"))),
    }
}

pub(crate) fn entry_status_from_db(raw: &str) -> sqlx::Result<EntryStatus> {
    match raw {
        "DRAFT" => Ok(EntryStatus::Draft),
        "LOCKED" => Ok(EntryStatus::Locked),
        other => Err(decode(format!(
            "unknown tournament_entries.status {other:?}"
        ))),
    }
}

/// The constant name the column stores.
pub(crate) fn entry_status_to_db(status: EntryStatus) -> &'static str {
    match status {
        EntryStatus::Draft => "DRAFT",
        EntryStatus::Locked => "LOCKED",
    }
}

// ---------------------------------------------------------------------------
// Roster swaps.
//
// Nothing here stores an allowance or a "swaps used" counter. Both numbers the
// feature needs are counted off the log at read time, for the same reason a
// points total is: a stored counter is a cache, and this one would have to be
// invalidated by an admin retuning `swaps_per_round` with a bare UPDATE, which
// announces nothing.
// ---------------------------------------------------------------------------

/// How many exchanges this entry has ever made.
///
/// The subtrahend in [`umfl_domain::roster_policy::swap_allowance`]. Counting
/// every round rather than the current one is what makes an unused window carry
/// over with nothing having to record that it went unused.
pub async fn swap_count_for_entry(db: impl PgExecutor<'_>, entry_id: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        r#"select count(*) as "count!" from roster_swaps where entry_id = $1"#,
        entry_id
    )
    .fetch_one(db)
    .await
}

/// Whether this entry already submitted its swaps for the given round.
///
/// A window grants one submission, not a budget to spend a click at a time, so
/// this is the gate behind `ALREADY_SWAPPED_THIS_ROUND`. An `exists` rather
/// than a count: the question is boolean and the answer stops at the first row.
pub async fn swapped_in_round(
    db: impl PgExecutor<'_>,
    entry_id: i64,
    round: i32,
) -> sqlx::Result<bool> {
    sqlx::query_scalar!(
        r#"select exists(
               select 1 from roster_swaps where entry_id = $1 and round = $2
           ) as "exists!""#,
        entry_id,
        round
    )
    .fetch_one(db)
    .await
}

/// Every entry's swap log for one tournament, keyed by entry id.
///
/// One query for the whole board rather than one per row: the standings fold
/// needs this beside `rosters`, and a per-entry query there would reintroduce
/// exactly the N+1 the read/write split exists to avoid.
///
/// Ordered by `(round, id)` because the replay in
/// [`umfl_domain::standings::EntryRoster::holdings`] walks it both ways --
/// two exchanges in the same round must be undone in the reverse of the order
/// they were made.
pub async fn swaps_by_entry_for_tournament(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
) -> sqlx::Result<IndexMap<i64, Vec<RosterSwap>>> {
    let rows = sqlx::query!(
        r#"select s.entry_id, s.round, s.hero_out_id, s.hero_in_id
           from roster_swaps s
               join tournament_entries e on e.id = s.entry_id
           where e.tournament_id = $1
           order by s.entry_id, s.round, s.id"#,
        tournament_id
    )
    .fetch_all(db)
    .await?;

    let mut by_entry: IndexMap<i64, Vec<RosterSwap>> = IndexMap::new();
    for row in rows {
        by_entry.entry(row.entry_id).or_default().push(RosterSwap {
            round: row.round,
            hero_out_id: row.hero_out_id,
            hero_in_id: row.hero_in_id,
        });
    }
    Ok(by_entry)
}

fn decode(message: String) -> sqlx::Error {
    sqlx::Error::Decode(message.into())
}
