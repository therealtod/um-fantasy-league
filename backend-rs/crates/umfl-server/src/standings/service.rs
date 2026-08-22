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
pub async fn board(state: &AppState, tournament_id: i64) -> ApiResult<StandingsBoard> {
    let mut tx = snapshot(state).await?;

    let rules = resolve_rules(&mut tx, tournament_id).await?;
    let matches = state
        .match_cache
        .find_by_tournament(&state.pool, tournament_id)
        .await?;
    let rosters = query::rosters(&mut *tx, tournament_id).await?;

    tx.commit().await?;
    Ok(fold::board(tournament_id, &matches, &rules, &rosters))
}

/// The newest recorded matches, as the Standings ticker renders them.
///
/// `since_match_id` is the polling key rather than a timestamp: parallel tables
/// in a round share a `played_at`, while the match id is a monotonic
/// `bigserial`. See the isolation note on [`board`] -- the same cross-statement
/// race applies here.
pub async fn ticker(
    state: &AppState,
    tournament_id: i64,
    since_match_id: i64,
    limit: usize,
) -> ApiResult<Vec<TickerEntry>> {
    let mut tx = snapshot(state).await?;

    let rules = resolve_rules(&mut tx, tournament_id).await?;
    let matches = state
        .match_cache
        .find_by_tournament_since(&state.pool, tournament_id, since_match_id, limit)
        .await?;

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
