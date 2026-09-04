//! The read side of the Live Standings screen: who entered, and what they
//! drafted.
//!
//! Oracle: `standings/StandingsQuery.kt`.
//!
//! Scoring is deliberately *not* in this query. Points are folded in
//! [`umfl_domain::standings`] because the coefficients live in
//! `scoring_coefficients` and each (hero, match) pair is then priced exactly
//! once, no matter how many rosters hold that hero -- cheaper than the join's
//! fan-out, and impossible to get inconsistent with the ticker.

use indexmap::IndexMap;
use sqlx::PgExecutor;
use umfl_domain::standings::{EntryRoster, RosterHero};

/// Every **locked** entry in the tournament with its roster, ordered by entry
/// then slot.
///
/// `where e.status = 'LOCKED'` is deliberate, not incidental: an entry that
/// never locked in a roster can never score a point once its tournament goes
/// live (see `umfl_domain::roster_policy::validate_lock`'s `TournamentClosed`
/// rule), so it has nothing to show on the board.
/// `crate::tournament::admin_service::update` already deletes these entries
/// the moment a tournament is saved as LIVE
/// (`crate::tournament::service::purge_unlocked_entries`); this filter is the
/// second line of defense that invariant describes, in case that purge is
/// ever skipped or fails -- not the only guard against an unlocked entry
/// appearing here.
///
/// Note the **left join** onto `entry_slots` regardless: a locked entry is
/// guaranteed a full roster by `RosterRule::IncompleteRoster`, but an inner
/// join would silently drop any entry that reaches this query without one --
/// exactly the case the filter above exists to guard against -- so this query
/// never depends on that guarantee holding. The pickless-entry case is
/// asserted in `tests/it/standings.rs`.
///
/// `th.cost` is left-joined for the same reason it is not snapshotted onto
/// `entry_slots`: the price is this tournament's, live. A hero that has since
/// left the pool has no `tournament_heroes` row at all, which is a 0 here rather
/// than a crash -- the Kotlin gets that from `ResultSet.getInt` returning 0 for
/// SQL NULL, and `unwrap_or(0)` below is that behaviour written out.
pub async fn rosters(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
) -> sqlx::Result<Vec<EntryRoster>> {
    let rows = sqlx::query!(
        // `as "…?"`: every column past `credit_grant` comes from the left-joined
        // side, so sqlx's non-null inference from the base tables is wrong here.
        r#"select e.id            as entry_id,
                  mg.id           as manager_id,
                  mg.handle,
                  mg.display_name,
                  e.credit_grant,
                  es.slot_index   as "slot_index?",
                  h.id            as "hero_id?",
                  h.name          as "hero_name?",
                  th.cost         as "hero_cost?"
           from tournament_entries e
               join managers mg on mg.id = e.manager_id
               left join entry_slots es on es.entry_id = e.id
               left join heroes h on h.id = es.hero_id
               left join tournament_heroes th
                   on th.tournament_id = e.tournament_id and th.hero_id = es.hero_id
           where e.tournament_id = $1 and e.status = 'LOCKED'
           order by e.id, es.slot_index"#,
        tournament_id
    )
    .fetch_all(db)
    .await?;

    // Kotlin's `groupBy`, which is a LinkedHashMap: first-encounter order, and
    // the query has already ordered that by entry id (PORTING.md §4.2).
    let mut by_entry: IndexMap<i64, EntryRoster> = IndexMap::new();
    for row in rows {
        let entry = by_entry.entry(row.entry_id).or_insert_with(|| EntryRoster {
            entry_id: row.entry_id,
            manager_id: row.manager_id,
            handle: row.handle,
            display_name: row.display_name,
            credit_grant: row.credit_grant,
            heroes: Vec::new(),
        });
        // The hero id is the presence check, exactly as in the Kotlin: it is
        // null for an entry with no slots, and the slot's own columns are then
        // null with it.
        if let (Some(hero_id), Some(name)) = (row.hero_id, row.hero_name) {
            entry.heroes.push(RosterHero {
                slot_index: row.slot_index.unwrap_or(0),
                hero_id,
                name,
                cost: row.hero_cost.unwrap_or(0),
            });
        }
    }
    Ok(by_entry.into_values().collect())
}
