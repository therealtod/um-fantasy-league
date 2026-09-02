//! The admin write path for match results, and the two admin reads beside it.
//!
//! Oracle: `match/AdminMatchService.kt` and the read halves of
//! `api/AdminMatchController.kt`. Every `@Transactional` method there is a
//! `pool.begin()` here and nothing else is (PORTING.md §7).
//!
//! Everything downstream -- standings, ticker, points -- is derived at read
//! time from what this saves, so recording, correcting or retracting a match is
//! the entire surface area; no total is ever recomputed and stored.
//!
//! Because this is the *only* writer of `tournament_matches` and its children,
//! the announcement all three write methods make is a complete account of when
//! that data changes. In Kotlin that announcement is a `StandingsUpdateEvent`
//! with two listeners; here it is [`announce`], [`announce_completed`] and
//! [`announce_committed`], and
//! the phases are the same. **Keep announcing from any method added here that
//! writes a match.**

use sqlx::PgConnection;
use std::collections::BTreeSet;
use umfl_domain::match_policy::{self, MatchBanInput, MatchGameInput, MatchParticipantInput};
use umfl_domain::match_result::MatchResult;
use umfl_domain::{DomainError, Violation};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::tournament::service::require_tournament;

use super::{
    HeroBanWrite, HeroPickWrite, MatchGameParticipantWrite, MatchGameWrite, MatchParticipantWrite,
    TournamentMatchWrite, query, writer,
};

/// Named in `V1__core_schema.sql`; Postgres reports it as the violated
/// constraint.
const LINK_INDEX: &str = "uq_tournament_match_external_link";

/// Recorded matches, newest first, optionally narrowed to one round.
pub async fn list(
    state: &AppState,
    tournament_id: i64,
    round: Option<i32>,
    limit: i64,
) -> ApiResult<Vec<MatchResult>> {
    let mut conn = state.pool.acquire().await?;
    require_tournament(&mut *conn, tournament_id).await?;
    Ok(query::find_by_tournament_newest_first(&mut conn, tournament_id, round, limit).await?)
}

/// One match, 404 unless it belongs to the tournament in the path.
pub async fn get(state: &AppState, tournament_id: i64, match_id: i64) -> ApiResult<MatchResult> {
    let mut conn = state.pool.acquire().await?;
    require_tournament(&mut *conn, tournament_id).await?;
    query::find_by_id(&mut conn, match_id)
        .await?
        .filter(|m| m.tournament_id == tournament_id)
        .ok_or_else(|| not_found(tournament_id, match_id))
}

#[allow(clippy::too_many_arguments)]
pub async fn record(
    state: &AppState,
    tournament_id: i64,
    round: i32,
    played_at: chrono::DateTime<chrono::Utc>,
    external_link: &str,
    participants: &[MatchParticipantInput],
    games: &[MatchGameInput],
    bans: &[MatchBanInput],
) -> ApiResult<MatchResult> {
    let mut tx = state.pool.begin().await?;
    require_tournament(&mut *tx, tournament_id).await?;
    validate(&mut tx, tournament_id, participants, games, bans).await?;

    let link = external_link.trim();
    require_link_unused(&mut tx, tournament_id, link, None).await?;

    let write = TournamentMatchWrite {
        id: None,
        tournament_id,
        round,
        played_at,
        external_link: link.to_owned(),
        participants: to_participants(participants),
        games: to_games(games),
        bans: to_bans(bans),
        picks: to_picks(participants),
    };
    let match_id = writer::insert(&mut tx, &write)
        .await
        .map_err(|e| link_conflict_or(e, link))?;

    announce(state, tournament_id);
    // This read is inside the same transaction that just inserted `match_id`,
    // so `None` here can only mean a bug in this service, never a race with
    // another writer -- nothing else can see the row until we commit. It
    // still answers 500 either way; the point of `ok_or_else` over the
    // `.expect()` this replaced is that it does so without unwinding through
    // `CatchPanicLayer` (see `http::panic_response`) and dropping a live
    // transaction mid-flight, and the log line below names the ids a bare
    // panic message would not have reached anyone with.
    //
    // Returning early here instead of falling through to `announce_completed`
    // is safe: `tx` is never committed on this path, so it rolls back on drop
    // and the insert above is undone. A reader who raced the window between
    // `announce` and the rollback sees exactly the pre-write rows -- which,
    // because the write never actually took effect, is not stale, so there is
    // no second invalidation for `announce_completed` to buy. (The pre-existing
    // `?` on the line above, for a genuine query failure rather than a missing
    // row, already relied on the same reasoning.)
    let saved = query::find_by_id(&mut tx, match_id).await?.ok_or_else(|| {
        tracing::error!(
            tournament_id,
            match_id,
            "Just-saved match not found by its own transaction"
        );
        ApiError::Internal
    })?;
    let outcome = tx.commit().await;
    announce_completed(state, tournament_id);
    outcome?;
    announce_committed(state, tournament_id);
    Ok(saved)
}

#[allow(clippy::too_many_arguments)]
pub async fn correct(
    state: &AppState,
    tournament_id: i64,
    match_id: i64,
    round: i32,
    played_at: chrono::DateTime<chrono::Utc>,
    external_link: &str,
    participants: &[MatchParticipantInput],
    games: &[MatchGameInput],
    bans: &[MatchBanInput],
) -> ApiResult<MatchResult> {
    let mut tx = state.pool.begin().await?;
    require_match(&mut tx, tournament_id, match_id).await?;
    validate(&mut tx, tournament_id, participants, games, bans).await?;

    let link = external_link.trim();
    require_link_unused(&mut tx, tournament_id, link, Some(match_id)).await?;

    let write = TournamentMatchWrite {
        id: Some(match_id),
        tournament_id,
        round,
        played_at,
        external_link: link.to_owned(),
        participants: to_participants(participants),
        games: to_games(games),
        bans: to_bans(bans),
        picks: to_picks(participants),
    };
    writer::update(&mut tx, &write)
        .await
        .map_err(|e| link_conflict_or(e, link))?;

    announce(state, tournament_id);
    // Same reasoning as the identical read in `record`: this transaction just
    // wrote `match_id` itself, so `None` is this service's own bug rather
    // than a race, a controlled 500 beats a panic unwinding a live
    // transaction, and skipping `announce_completed` on this early return is
    // safe because the uncommitted write it would be protecting a reader from
    // never survives the rollback that not calling `tx.commit()` triggers.
    let saved = query::find_by_id(&mut tx, match_id).await?.ok_or_else(|| {
        tracing::error!(
            tournament_id,
            match_id,
            "Just-corrected match not found by its own transaction"
        );
        ApiError::Internal
    })?;
    let outcome = tx.commit().await;
    announce_completed(state, tournament_id);
    outcome?;
    announce_committed(state, tournament_id);
    Ok(saved)
}

pub async fn delete(state: &AppState, tournament_id: i64, match_id: i64) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    require_match(&mut tx, tournament_id, match_id).await?;
    writer::delete(&mut tx, match_id).await?;

    announce(state, tournament_id);
    let outcome = tx.commit().await;
    announce_completed(state, tournament_id);
    outcome?;
    announce_committed(state, tournament_id);
    Ok(())
}

/// The first half of the Kotlin's `StandingsUpdateEvent` pair: fires
/// **immediately, inside the writing transaction**, so a reader on this
/// connection is not served the list as it stood before the write.
///
/// The standings SSE hub must **not** be signalled here. It listens to the same
/// event `AFTER_COMMIT`, because telling browsers "something changed" about a
/// write that then rolls back would be a lie; the cache is invalidated in both
/// phases precisely because a rollback un-writes rows it may already hold.
fn announce(state: &AppState, tournament_id: i64) {
    state.match_cache.invalidate(tournament_id);
}

/// The second half: fires once the transaction has **ended**, committed or
/// rolled back. See [`super::cache`] for why neither half is sufficient alone,
/// and why this one is completion rather than commit.
fn announce_completed(state: &AppState, tournament_id: i64) {
    state.match_cache.invalidate(tournament_id);
}

/// The standings push, and the reason it is a *third* call rather than a line
/// inside [`announce_completed`]: it is **commit-only**.
///
/// Kotlin gets the distinction from two listeners on one `StandingsUpdateEvent`
/// -- `MatchResultCache`'s is `AFTER_COMPLETION`, `StandingsSseHub`'s is
/// `AFTER_COMMIT`. Telling browsers "something changed" about a write that
/// rolled back would be a lie, whereas a rollback un-writes rows the cache may
/// already hold and so invalidates just as surely as a commit does. Hence the
/// position: after the `outcome?`, which is the only place a rolled-back write
/// has already returned.
fn announce_committed(state: &AppState, tournament_id: i64) {
    state.standings_hub.notify(tournament_id);
}

/// The source link is what stops the same match being imported twice, so a link
/// already spent in this tournament is refused rather than warned about -- a
/// duplicated match silently double-counts every appearance, win and ban it
/// carries, and nothing surfaces it until someone doubts the standings.
///
/// `correcting_match_id` is the match being corrected, excluded from the
/// search: re-saving a match under the URL it already holds is the ordinary
/// correction path and updates the row in place. Only moving one match's link
/// onto another's is a conflict.
async fn require_link_unused(
    conn: &mut PgConnection,
    tournament_id: i64,
    link: &str,
    correcting_match_id: Option<i64>,
) -> ApiResult<()> {
    let collision = query::find_id_by_external_link(&mut *conn, tournament_id, link).await?;
    match collision {
        Some(id) if Some(id) != correcting_match_id => Err(DomainError::conflict(format!(
            "Match {id} is already recorded against {link}. \
             Correct that match instead of recording a second one."
        ))
        .into()),
        _ => Ok(()),
    }
}

/// The check above and the write are two statements, so a match recorded in
/// between slips past the check and is stopped by
/// `uq_tournament_match_external_link` instead. That window is narrow but real,
/// so the violation is translated into a conflict naming the link rather than
/// left to the data-integrity backstop, which renders the generic "should never
/// fire" 409 and names nothing the admin can act on. Mirrors
/// [`crate::map::admin_service::remove_from_pool`].
///
/// The offending match cannot be named here the way [`require_link_unused`]
/// names it: the failed insert has already aborted the transaction, so no
/// further query can run inside it. Only that one index is translated -- every
/// other integrity violation is still a bug worth surfacing as itself, not
/// mislabelled a duplicate link.
fn link_conflict_or(err: sqlx::Error, link: &str) -> ApiError {
    let is_link_index = matches!(
        &err,
        sqlx::Error::Database(db) if db.constraint() == Some(LINK_INDEX)
    );
    if is_link_index {
        DomainError::conflict(format!(
            "Another match in this tournament was just recorded against {link}. \
             Correct that match instead of recording a second one."
        ))
        .into()
    } else {
        ApiError::from_sqlx(err)
    }
}

/// The match under correction or deletion, confirmed to be in this tournament.
///
/// A bare header read: the Kotlin loads the whole write aggregate here, but the
/// only things it goes on to use are the id and the tournament it belongs to --
/// `correct` replaces every child collection wholesale and `delete` cascades.
async fn require_match(
    conn: &mut PgConnection,
    tournament_id: i64,
    match_id: i64,
) -> ApiResult<()> {
    let owner = sqlx::query_scalar!(
        "select tournament_id from tournament_matches where id = $1",
        match_id
    )
    .fetch_optional(conn)
    .await?;
    match owner {
        Some(owner) if owner == tournament_id => Ok(()),
        _ => Err(not_found(tournament_id, match_id)),
    }
}

fn not_found(tournament_id: i64, match_id: i64) -> ApiError {
    DomainError::not_found(format!("No match {match_id} in tournament {tournament_id}")).into()
}

/// The pure policy, with its inputs read in.
///
/// `valid_hero_ids` is the set of *referenced* ids that actually exist, not the
/// whole catalogue: the policy reports an unknown hero, so it only needs to
/// know which of the ids this submission names are real.
async fn validate(
    conn: &mut PgConnection,
    tournament_id: i64,
    participants: &[MatchParticipantInput],
    games: &[MatchGameInput],
    bans: &[MatchBanInput],
) -> ApiResult<()> {
    let referenced: Vec<i64> = games
        .iter()
        .flat_map(|g| g.participants.iter().map(|p| p.hero_id))
        .chain(
            participants
                .iter()
                .flat_map(|p| p.drafted_hero_ids.iter().copied()),
        )
        .chain(bans.iter().map(|b| b.hero_id))
        .collect();

    let valid_map_ids: BTreeSet<i64> = crate::map::query::pool_map_ids(&mut *conn, tournament_id)
        .await?
        .into_iter()
        .collect();
    let valid_hero_ids: BTreeSet<i64> =
        sqlx::query_scalar!("select id from heroes where id = any($1)", &referenced)
            .fetch_all(conn)
            .await?
            .into_iter()
            .collect();

    let violations =
        match_policy::validate(&valid_map_ids, &valid_hero_ids, participants, games, bans);
    if violations.is_empty() {
        return Ok(());
    }
    Err(ApiError::Domain(DomainError::MatchRule(
        violations.into_iter().map(Violation::from).collect(),
    )))
}

/// "Absent" for optional free text is `None`, never `""`. An admin form posts
/// an untouched text input as an empty string, and `non_null` inclusion only
/// drops nulls -- so without this a blank box comes back out of the API as `""`
/// and a consumer testing for null sees a player who was never entered.
fn blank_to_none(text: Option<&String>) -> Option<String> {
    text.map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

fn to_participants(participants: &[MatchParticipantInput]) -> Vec<MatchParticipantWrite> {
    participants
        .iter()
        .map(|p| MatchParticipantWrite {
            player_label: blank_to_none(p.player_label.as_ref()),
        })
        .collect()
}

fn to_games(games: &[MatchGameInput]) -> Vec<MatchGameWrite> {
    games
        .iter()
        .map(|g| MatchGameWrite {
            game_number: g.game_number,
            map_id: g.map_id,
            participants: g
                .participants
                .iter()
                .map(|p| MatchGameParticipantWrite {
                    hero_id: p.hero_id,
                    health_remaining: p.health_remaining,
                    is_winner: p.is_winner,
                })
                .collect(),
        })
        .collect()
}

/// The Kotlin collects into a `Set`, which would drop an exactly-repeated ban.
/// Nothing reaches here to drop: `MatchRule::DuplicateBan` has already rejected
/// a hero struck twice, whatever the type or side.
fn to_bans(bans: &[MatchBanInput]) -> Vec<HeroBanWrite> {
    bans.iter()
        .map(|b| HeroBanWrite {
            hero_id: b.hero_id,
            ban_type: b.ban_type,
            side: b.side,
        })
        .collect()
}

/// The draft rides in on the participants -- a pick belongs to a side -- but
/// persists as a child of the match, since `match_participants` is
/// composite-keyed and cannot own children of its own. The side is the
/// participant's list position, the same ordinal `match_participants.side` is
/// written from.
///
/// The per-side `distinct()` is the Kotlin's and is load-bearing: a side that
/// names the same hero twice is not a `DUPLICATE_PICK` across sides and would
/// otherwise meet the `(match_id, side, hero_id)` primary key.
fn to_picks(participants: &[MatchParticipantInput]) -> Vec<HeroPickWrite> {
    let mut picks = Vec::new();
    for (side, participant) in participants.iter().enumerate() {
        let mut seen = BTreeSet::new();
        for hero_id in &participant.drafted_hero_ids {
            if seen.insert(*hero_id) {
                picks.push(HeroPickWrite {
                    side: side as i32,
                    hero_id: *hero_id,
                });
            }
        }
    }
    picks
}
