//! Tournament and entry reads.
//!
//! Oracle: `tournament/TournamentRepositories.kt` (the derived-query and
//! `@Query` halves) and `tournament/TournamentEntryQuery.kt`.
//!
//! The Kotlin splits these across a Spring Data `CrudRepository` and a
//! hand-written `JdbcClient` projection. Here they are all `query!` reads in the
//! file the naming convention reserves for reads (PORTING.md §3); the *write*
//! half of `TournamentEntryRepository.save` is `writer.rs`, which is the split
//! the class names were carrying.

use indexmap::IndexMap;
use sqlx::PgExecutor;
use umfl_domain::tournament::{
    EntrySlot, EntryStatus, Tournament, TournamentEntry, TournamentFormat, TournamentStatus,
};

/// `findAllByOrderByStartDateAsc`.
///
/// Ties are left to the database, exactly as Spring Data's derived query left
/// them: adding an id tiebreak here would be a behaviour change smuggled into a
/// port, and both runtimes read the same Postgres.
pub async fn find_all_ordered(db: impl PgExecutor<'_>) -> sqlx::Result<Vec<Tournament>> {
    let rows = sqlx::query!(
        r#"select id, name, format, status, start_date, end_date,
                  capacity, roster_size, credit_grant
           from tournament order by start_date asc"#
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
                  capacity, roster_size, credit_grant
           from tournament where status = $1 order by start_date asc"#,
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
            })
        })
        .collect()
}

pub async fn find_by_id(db: impl PgExecutor<'_>, id: i64) -> sqlx::Result<Option<Tournament>> {
    let Some(r) = sqlx::query!(
        r#"select id, name, format, status, start_date, end_date,
                  capacity, roster_size, credit_grant
           from tournament where id = $1"#,
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
/// writes `tournament`.
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
        "select capacity from tournament where id = $1 for update",
        id
    )
    .fetch_optional(db)
    .await
}

/// `countByTournamentId`.
pub async fn count_entries(db: impl PgExecutor<'_>, tournament_id: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        r#"select count(*) as "count!" from tournament_entry where tournament_id = $1"#,
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
           from tournament t
           left join tournament_entry e on e.tournament_id = t.id
           group by t.id"#
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.tournament_id, r.entry_count))
        .collect())
}

/// `findByTournamentIdAndManagerId`, loading the aggregate whole.
///
/// Spring Data JDBC populated `slots` with a second query keyed on
/// `entry_id`; so does this, and it keeps that query's `order by slot_index`
/// because the list position **is** the slot index. See the `EntrySlot` doc in
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
           from tournament_entry where tournament_id = $1 and manager_id = $2"#,
        tournament_id,
        manager_id
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };

    let slots = sqlx::query_scalar!(
        "select hero_id from entry_slot where entry_id = $1 order by slot_index",
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
/// costs a second query per entry to populate `entry_slot`.
pub async fn statuses_by_tournament(
    db: impl PgExecutor<'_>,
    manager_id: i64,
) -> sqlx::Result<IndexMap<i64, EntryStatus>> {
    let rows = sqlx::query!(
        "select tournament_id, status from tournament_entry where manager_id = $1",
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
        "select status from tournament_entry where manager_id = $1 and tournament_id = $2",
        manager_id,
        tournament_id
    )
    .fetch_optional(db)
    .await?;
    row.map(|s| entry_status_from_db(&s)).transpose()
}

// The three text columns below carry CHECK constraints naming exactly these
// values, so an unknown one means the schema moved underneath the code. That is
// a decode failure rather than a panic: `From<sqlx::Error>` renders it as the
// 500 a Kotlin `valueOf` would also have produced, and says which column.

fn format_from_db(raw: &str) -> sqlx::Result<TournamentFormat> {
    match raw {
        "BANQUEST" => Ok(TournamentFormat::Banquest),
        "ARSENAL" => Ok(TournamentFormat::Arsenal),
        other => Err(decode(format!("unknown tournament.format {other:?}"))),
    }
}

fn status_from_db(raw: &str) -> sqlx::Result<TournamentStatus> {
    match raw {
        "SCHEDULED" => Ok(TournamentStatus::Scheduled),
        "REGISTRATION_OPEN" => Ok(TournamentStatus::RegistrationOpen),
        "LIVE" => Ok(TournamentStatus::Live),
        "COMPLETED" => Ok(TournamentStatus::Completed),
        other => Err(decode(format!("unknown tournament.status {other:?}"))),
    }
}

pub(crate) fn entry_status_from_db(raw: &str) -> sqlx::Result<EntryStatus> {
    match raw {
        "DRAFT" => Ok(EntryStatus::Draft),
        "LOCKED" => Ok(EntryStatus::Locked),
        other => Err(decode(format!("unknown tournament_entry.status {other:?}"))),
    }
}

/// The Kotlin constant name, which is what the column stores.
pub(crate) fn entry_status_to_db(status: EntryStatus) -> &'static str {
    match status {
        EntryStatus::Draft => "DRAFT",
        EntryStatus::Locked => "LOCKED",
    }
}

fn decode(message: String) -> sqlx::Error {
    sqlx::Error::Decode(message.into())
}
