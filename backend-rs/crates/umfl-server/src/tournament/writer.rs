//! Entry writes: an entry and its `entry_slots` as one aggregate.
//!
//! Saving that aggregate does one of two things:
//!
//! * **New entry** (no id yet): insert the root, then insert each slot with
//!   the generated id and the list position as `slot_index`.
//! * **Existing entry**: update the root, **delete every slot row**, then
//!   re-insert them, rather than diffing them against what's there. That
//!   delete-and-reinsert is what makes reordering a roster work at all —
//!   `entry_slots` is keyed `(entry_id, slot_index)`, so an in-place update of
//!   two swapped rows would collide with the unique key mid-statement.
//!
//! Every function takes the connection rather than an executor: each is more
//! than one statement and all of them belong to somebody's transaction.

use sqlx::{PgConnection, PgExecutor};
use umfl_domain::tournament::{Tournament, TournamentEntry};

use super::query::{entry_status_to_db, format_to_db, status_to_db};

// ---------------------------------------------------------------------------
// The tournament root itself -- inserts when the id is absent and updates
// when it is not. Unlike the entry above, `tournaments` owns no child
// collection, so there is no delete-and-reinsert cascade here -- see
// `map::writer` and `hero::writer` for the same shape on a childless root.
// ---------------------------------------------------------------------------

/// Inserts a tournament, returning the generated id.
pub async fn insert_tournament(db: impl PgExecutor<'_>, t: &Tournament) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        r#"insert into tournaments
               (name, format, status, start_date, end_date, capacity, roster_size, credit_grant,
                current_round, swaps_per_round, swap_window_open)
           values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
           returning id"#,
        t.name,
        format_to_db(t.format),
        status_to_db(t.status),
        t.start_date,
        t.end_date,
        t.capacity,
        t.roster_size,
        t.credit_grant,
        t.current_round,
        t.swaps_per_round,
        t.swap_window_open
    )
    .fetch_one(db)
    .await
}

/// Full replace, including `status` -- there is no status-transition state
/// machine: an admin is trusted to move a tournament through its lifecycle
/// sensibly.
///
/// This writes every column, `current_round` and `swap_window_open` included.
/// Keeping an admin's edit from resetting those is
/// [`super::admin_service::update`]'s job, which carries them over off the
/// loaded row -- the writer stays an honest whole-row write rather than
/// growing a list of columns it quietly declines to touch.
///
/// # Panics
///
/// On a tournament with no id. Unreachable -- every caller loaded it from
/// [`super::query::find_by_id`] -- and asserted here as a defensive invariant
/// at the same point.
pub async fn update_tournament(db: impl PgExecutor<'_>, t: &Tournament) -> sqlx::Result<()> {
    let id = t.id.expect("a loaded tournament has an id");
    sqlx::query!(
        r#"update tournaments
              set name = $2, format = $3, status = $4, start_date = $5,
                  end_date = $6, capacity = $7, roster_size = $8, credit_grant = $9,
                  current_round = $10, swaps_per_round = $11, swap_window_open = $12
            where id = $1"#,
        id,
        t.name,
        format_to_db(t.format),
        status_to_db(t.status),
        t.start_date,
        t.end_date,
        t.capacity,
        t.roster_size,
        t.credit_grant,
        t.current_round,
        t.swaps_per_round,
        t.swap_window_open
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Records the exchanges one submission made, as one statement.
///
/// `swapped_at` is left to the column's `default now()`, unlike
/// `registered_at` on an entry: no service builds that value and no response
/// carries it back, so the statement's own clock is the only clock there is.
pub async fn insert_roster_swaps(
    conn: &mut PgConnection,
    entry_id: i64,
    round: i32,
    exchanges: &[(i64, i64)],
) -> sqlx::Result<()> {
    if exchanges.is_empty() {
        return Ok(());
    }
    let out_ids: Vec<i64> = exchanges.iter().map(|(out, _)| *out).collect();
    let in_ids: Vec<i64> = exchanges.iter().map(|(_, into)| *into).collect();
    sqlx::query!(
        r#"insert into roster_swaps (entry_id, round, hero_out_id, hero_in_id)
           select $1, $2, outs[i], ins[i]
           from (select $3::bigint[] as outs, $4::bigint[] as ins) s,
                generate_subscripts(s.outs, 1) as i"#,
        entry_id,
        round,
        &out_ids,
        &in_ids
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// Moves the tournament into its next round.
///
/// A bare increment rather than a set-to-N: the admin's intent is "the round is
/// over", and reading the current value into the application to add one would
/// let two admins racing on the same tournament both write the same number.
/// Returns the round now current.
pub async fn advance_current_round(db: impl PgExecutor<'_>, id: i64) -> sqlx::Result<i32> {
    sqlx::query_scalar!(
        "update tournaments set current_round = current_round + 1 where id = $1
         returning current_round",
        id
    )
    .fetch_one(db)
    .await
}

/// Opens or shuts the swap window.
pub async fn set_swap_window_open(
    db: impl PgExecutor<'_>,
    id: i64,
    open: bool,
) -> sqlx::Result<()> {
    sqlx::query!(
        "update tournaments set swap_window_open = $2 where id = $1",
        id,
        open
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Deletes a tournament. Every foreign key onto `tournaments` is `on delete
/// cascade` (see `V1__core_schema.sql`), so this alone removes its hero
/// pool, board pool, entries (and their slots), scoring rule sets (and
/// their coefficients), and matches (and their participants, games and
/// bans) -- see `AdminTournamentService.delete`'s doc for the itemised list.
pub async fn delete_tournament(db: impl PgExecutor<'_>, id: i64) -> sqlx::Result<()> {
    sqlx::query!("delete from tournaments where id = $1", id)
        .execute(db)
        .await?;
    Ok(())
}

/// Inserts a new entry and its slots, returning the generated id.
///
/// `registered_at` is written explicitly rather than left to the column's
/// `default now()`: the value is built in the service, and a column default
/// would be the *statement's* clock rather than the value the response
/// carries back.
pub async fn insert_entry(conn: &mut PgConnection, entry: &TournamentEntry) -> sqlx::Result<i64> {
    let id = sqlx::query_scalar!(
        r#"insert into tournament_entries
               (tournament_id, manager_id, status, credit_grant, registered_at, locked_at)
           values ($1, $2, $3, $4, $5, $6)
           returning id"#,
        entry.tournament_id,
        entry.manager_id,
        entry_status_to_db(entry.status),
        entry.credit_grant,
        entry.registered_at,
        entry.locked_at
    )
    .fetch_one(&mut *conn)
    .await?;

    insert_slots(conn, id, &entry.hero_ids()).await?;
    Ok(id)
}

/// Updates an existing entry: the root row, then its slots wholesale.
///
/// # Panics
///
/// On an entry with no id. Unreachable — every caller loaded the entry from
/// [`super::query::find_entry`] — and asserted here as a defensive invariant
/// at each of these call sites.
pub async fn update_entry(conn: &mut PgConnection, entry: &TournamentEntry) -> sqlx::Result<()> {
    let id = entry.id.expect("a loaded entry has an id");

    sqlx::query!(
        r#"update tournament_entries
              set tournament_id = $2, manager_id = $3, status = $4,
                  credit_grant = $5, registered_at = $6, locked_at = $7
            where id = $1"#,
        id,
        entry.tournament_id,
        entry.manager_id,
        entry_status_to_db(entry.status),
        entry.credit_grant,
        entry.registered_at,
        entry.locked_at
    )
    .execute(&mut *conn)
    .await?;

    sqlx::query!("delete from entry_slots where entry_id = $1", id)
        .execute(&mut *conn)
        .await?;
    insert_slots(conn, id, &entry.hero_ids()).await?;
    Ok(())
}

/// Deletes every entry in the tournament that never locked in a roster.
pub async fn delete_unlocked_entries(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
) -> sqlx::Result<u64> {
    let result = sqlx::query!(
        "delete from tournament_entries where tournament_id = $1 and status <> 'LOCKED'",
        tournament_id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

/// Writes the roster in list order, because the list index **is**
/// `entry_slots.slot_index`.
///
/// `unnest` with `generate_subscripts` rather than a loop: one statement, and
/// the index comes from the array's own position so nothing here can drift out
/// of step with the `Vec` the caller passed.
async fn insert_slots(
    conn: &mut PgConnection,
    entry_id: i64,
    hero_ids: &[i64],
) -> sqlx::Result<()> {
    if hero_ids.is_empty() {
        return Ok(());
    }
    sqlx::query!(
        r#"insert into entry_slots (entry_id, slot_index, hero_id)
           select $1, i - 1, ids[i]
           from (select $2::bigint[] as ids) s,
                generate_subscripts(s.ids, 1) as i"#,
        entry_id,
        hero_ids
    )
    .execute(conn)
    .await?;
    Ok(())
}
