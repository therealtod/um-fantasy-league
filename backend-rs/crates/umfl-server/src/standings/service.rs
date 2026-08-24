//! The snapshot the leaderboard is read under, and the three inputs the fold
//! takes.
//!
//! Oracle: `standings/StandingsService.kt` -- but only its *plumbing*. The
//! arithmetic is [`umfl_domain::standings`], where it is testable without
//! Postgres (PORTING.md §3a). What is left here is what the Kotlin's
//! `@Transactional(readOnly = true, isolation = REPEATABLE_READ)` did.
//!
//! Points are computed on every read and never stored. Coefficients and hero
//! costs are mutable reference data retuned with a bare UPDATE, so a stored
//! *total* would be a cache with nothing to invalidate it; at tournament scale
//! the fold is microseconds. What *is* cached is the fold's input, in
//! [`crate::r#match::MatchResultCache`] -- the assembled match list, which
//! unlike a total has exactly one writer and therefore a complete invalidation
//! signal. The rules and rosters read below are deliberately not cached for
//! precisely that reason, so re-pricing a hero, retuning a coefficient or
//! changing a roster is visible on the very next request.

use sqlx::{Executor, PgConnection, Postgres, Transaction};
use umfl_domain::scoring_engine::ScoringRules;
use umfl_domain::standings as fold;
use umfl_domain::standings::{StandingsBoard, TickerEntry};

use crate::error::ApiResult;
use crate::scoring::query as scoring_query;
use crate::state::AppState;

use super::query;

/// The leaderboard.
///
/// # Why REPEATABLE READ, and not merely read-only
///
/// Postgres's default READ COMMITTED gives every *statement* its own snapshot,
/// so a plain transaction still lets a concurrent
/// [`crate::r#match::admin_service`] `record`/`correct`/`delete` land between
/// this method's several statements -- the rules read, the match assembly's six
/// queries, the rosters read -- and skew them against each other. A delete
/// committing between the match header query and its participants query yields
/// a header whose games come back empty. REPEATABLE READ pins one snapshot for
/// the whole transaction. It needs no retry handling because the transaction is
/// read-only: Postgres raises a serialization failure only on a write/write
/// conflict, which a read-only transaction can never have.
///
/// The match cache narrows what that buys without making it redundant. On a hit
/// the list was assembled under an earlier reader's snapshot and is internally
/// coherent in the same way, just older. The guarantee is therefore "the match
/// list is a coherent snapshot", not "every fact on this board came from one
/// snapshot" -- the rules read and the roster read are one statement each and
/// describe facts uncorrelated with match writes, so nothing skews against
/// anything. The worst case is a board pricing a match list one write behind,
/// which is the staleness the cache is for and which the SSE push corrects.
///
/// # The cache read happens before the snapshot opens, and that order is load-bearing
///
/// `MatchResultCache::find_by_tournament` is called *first*, while this
/// function is not holding any pooled connection at all -- not after
/// `snapshot(state)`, even though the match list is exactly the input the
/// paragraph above says needs no correlation with the rules/roster reads. On a
/// cache hit the ordering costs nothing either way. On a *miss*, `load`
/// (`match/cache.rs`) calls `pool.acquire()` for a connection of its own,
/// separate from whatever this function is holding -- so nesting the cache
/// read inside `snapshot`'s transaction means every concurrent caller needs
/// **two** pooled connections at once to make progress: one held by its own
/// snapshot, one for its own cache load. Against `max_connections(10)`, ten
/// concurrent misses each hold one connection and each block acquiring a
/// second, and nothing left in the pool is ever going to hand one back --
/// a self-deadlock that resolves only when sqlx's acquire timeout fires. That
/// is exactly the burst this cache exists to absorb (one match write tells up
/// to 200 tabs per tournament to refetch, and each pulls both the board and
/// the ticker head), so the ordering is not a style choice: a reader who
/// "tidies" the cache call back inside the transaction reintroduces the
/// deadlock under the load the cache was built for. The regression is pinned
/// against a one-connection pool -- the smallest pool that expresses "no
/// spare connection available" -- by `tests/it/standings.rs`'s
/// `the_board_and_ticker_do_not_hold_a_connection_while_the_cache_loads`.
///
/// This ordering is also a prerequisite for PORTING.md §3b's open item --
/// giving `load` its own `repeatable read read only` transaction so a miss is
/// itself a coherent snapshot. Doing that while the call is still nested here
/// would make the nesting strictly worse: two transactions open on two
/// connections *simultaneously* for the whole load, per caller, rather than
/// today's two connections open only briefly and non-overlapping in the
/// common case. Closing that gap has to start from this function already
/// calling the cache before it opens anything of its own.
pub async fn board(state: &AppState, tournament_id: i64) -> ApiResult<StandingsBoard> {
    let matches = state
        .match_cache
        .find_by_tournament(&state.pool, tournament_id)
        .await?;

    let mut tx = snapshot(state).await?;
    let rules = resolve_rules(&mut tx, tournament_id).await?;
    let rosters = query::rosters(&mut *tx, tournament_id).await?;
    tx.commit().await?;

    Ok(fold::board(tournament_id, &matches, &rules, &rosters))
}

/// The newest recorded matches, as the Standings ticker renders them.
///
/// `since_match_id` is the polling key rather than a timestamp: parallel tables
/// in a round share a `played_at`, while the match id is a monotonic
/// `bigserial`. See the isolation note on [`board`] -- the same cross-statement
/// race applies here, and so does the cache-before-snapshot ordering: the
/// cache read below runs before [`snapshot`] opens a connection, for the
/// identical pool-self-deadlock reason documented there.
pub async fn ticker(
    state: &AppState,
    tournament_id: i64,
    since_match_id: i64,
    limit: usize,
) -> ApiResult<Vec<TickerEntry>> {
    let matches = state
        .match_cache
        .find_by_tournament_since(&state.pool, tournament_id, since_match_id, limit)
        .await?;

    let mut tx = snapshot(state).await?;
    let rules = resolve_rules(&mut tx, tournament_id).await?;
    tx.commit().await?;

    Ok(fold::ticker(&matches, &rules))
}

/// `BEGIN`, then the isolation level as the **very next statement**.
///
/// Anywhere later and Postgres silently leaves the transaction at READ
/// COMMITTED (PORTING.md §7) -- it is not an error to try, which is exactly why
/// this is a named helper both entry points call rather than a line each of
/// them could drift on.
async fn snapshot(state: &AppState) -> sqlx::Result<Transaction<'static, Postgres>> {
    let mut tx = state.pool.begin().await?;
    tx.execute("set transaction isolation level repeatable read read only")
        .await?;
    Ok(tx)
}

/// The tournament's active rules, with a note in the log for any metric this
/// build cannot price.
///
/// An unknown metric is never an error: `AGENTS.md` makes "unknown metrics
/// score zero and are dropped from the columns" a rule, and the seed's
/// `CROWD_FAVOURITE` is its standing proof.
async fn resolve_rules(conn: &mut PgConnection, tournament_id: i64) -> sqlx::Result<ScoringRules> {
    let rules = scoring_query::active_rules(&mut *conn, tournament_id).await?;
    if !rules.unknown_metrics().is_empty() {
        tracing::info!(
            tournament_id,
            metrics = ?rules.unknown_metrics(),
            "Tournament weights metric(s) that no extractor implements; they score zero."
        );
    }
    Ok(rules)
}
