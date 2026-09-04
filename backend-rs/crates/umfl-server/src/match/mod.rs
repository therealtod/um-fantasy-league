//! Recorded match results: the admin write path, and the reads everything
//! downstream folds.
//!
//! The module is `r#match` because `match` is a Rust keyword; the directory is
//! `match/`, and the `r#` prefix is only needed at each use site.
//!
//! **Nothing here stores a point.** A match is the raw fact; standings, the
//! ticker and every total are derived from these at read time
//! ([`umfl_domain::standings`]). Recording, correcting or retracting a match is
//! therefore the entire write surface for the leaderboard, which is also what
//! makes [`cache`]'s invalidation signal complete.

pub mod admin_service;
pub mod cache;
pub mod query;
pub mod writer;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use umfl_domain::match_policy::{
    MatchBanInput, MatchGameInput, MatchGameParticipantInput, MatchParticipantInput,
};
use umfl_domain::match_result::{
    BanResult, BanType, GameParticipantResult, GameResult, MatchParticipantResult, MatchResult,
};

use crate::auth::CurrentManager;
use crate::error::ApiResult;
use crate::http::extract::{AppPath, AppQuery, ValidJson};
use crate::state::AppState;

pub use cache::MatchResultCache;

// ---------------------------------------------------------------------------
// The write aggregate
// ---------------------------------------------------------------------------

/// A recorded match as [`writer`] saves it: the whole aggregate root, minus
/// any persistence annotations -- there is no ORM here to need them.
///
/// Read access never goes through this: [`query`] assembles
/// [`umfl_domain::match_result::MatchResult`] instead. This exists only so an
/// admin write can save participants, games and the draft (picks *and* bans)
/// together as one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentMatchWrite {
    pub id: Option<i64>,
    pub tournament_id: i64,
    pub round: i32,
    pub played_at: DateTime<Utc>,
    /// Required, and unique within the tournament --
    /// `uq_tournament_match_external_link` is what stops the same match being
    /// imported twice. A match with no page anywhere carries a synthetic
    /// `urn:umfl:match:<id>` placeholder instead.
    pub external_link: String,
    /// `side` (0 or 1) is the **list position**, written to
    /// `match_participants.side`.
    pub participants: Vec<MatchParticipantWrite>,
    pub games: Vec<MatchGameWrite>,
    pub bans: Vec<HeroBanWrite>,
    /// The picks half of the draft, to [`Self::bans`]' bans half.
    ///
    /// Hangs off the root rather than off a participant, where it would read
    /// more naturally: `match_participants` has a composite key, and
    /// `match_hero_picks` is keyed on `(match_id, side, hero_id)` rather than
    /// on a participant row, so grouping picks under the root matches the
    /// schema.
    pub picks: Vec<HeroPickWrite>,
}

/// One side of the series -- which human played it, for the whole match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchParticipantWrite {
    pub player_label: Option<String>,
}

/// One game within a series. `game_number` is real admin-meaningful data
/// (checked `> 0`), not a list-position ordinal, so it is an explicit field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchGameWrite {
    pub game_number: i32,
    pub map_id: i64,
    /// `side` is the list position again, as it is on the match itself.
    pub participants: Vec<MatchGameParticipantWrite>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchGameParticipantWrite {
    pub hero_id: i64,
    pub health_remaining: i32,
    pub is_winner: bool,
}

/// `(match_id, hero_id)` is the natural key -- a hero is struck at most once
/// per series however many sides wanted it. `side` is the draft it came out of,
/// while `ban_type` says who struck it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeroBanWrite {
    pub hero_id: i64,
    pub ban_type: BanType,
    pub side: Option<i32>,
}

/// A hero one side drafted for the series, played or not.
/// `(match_id, side, hero_id)` is the natural key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeroPickWrite {
    pub side: i32,
    pub hero_id: i64,
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// A recorded match on the wire.
///
/// Every field but `games` is the domain type verbatim. `games` is wrapped
/// only to carry `winner`; see [`GameResultDto`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchResultDto {
    pub match_id: i64,
    pub tournament_id: i64,
    pub round: i32,
    #[serde(with = "umfl_domain::time::java_instant")]
    pub played_at: DateTime<Utc>,
    pub external_link: String,
    pub participants: Vec<MatchParticipantResult>,
    pub games: Vec<GameResultDto>,
    pub bans: Vec<BanResult>,
}

impl From<MatchResult> for MatchResultDto {
    fn from(result: MatchResult) -> Self {
        Self {
            match_id: result.match_id,
            tournament_id: result.tournament_id,
            round: result.round,
            played_at: result.played_at,
            external_link: result.external_link,
            participants: result.participants,
            games: result.games.into_iter().map(GameResultDto::from).collect(),
            bans: result.bans,
        }
    }
}

/// One game, plus a `winner` field derived from it on the DTO rather than on
/// the domain type. `MatchListAdmin.vue:121` derives the winner itself and
/// never reads this field, so it is dead weight on the wire -- kept rather
/// than dropped without a deliberate decision to change the response shape.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameResultDto {
    pub game_id: i64,
    pub game_number: i32,
    pub map_id: i64,
    pub map_name: String,
    pub participants: Vec<GameParticipantResult>,
    /// Never absent for a recorded game: `MatchRule::NotExactlyOneWinner`
    /// rejects a submission whose game has anything other than exactly one
    /// winner, so there is no drawn game to represent. Skipped when `None`
    /// anyway, because `non_null` omits rather than emitting a `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winner: Option<GameParticipantResult>,
}

impl From<GameResult> for GameResultDto {
    fn from(game: GameResult) -> Self {
        let winner = game.winner().cloned();
        Self {
            game_id: game.game_id,
            game_number: game.game_number,
            map_id: game.map_id,
            map_name: game.map_name,
            participants: game.participants,
            winner,
        }
    }
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// Each rule below is a garde `custom` rather than a built-in, because the
/// message string is what the client renders and garde 0.23 cannot override a
/// built-in rule's wording. Where a field is both required and needs a second
/// check, one function covers both without ambiguity: the second check only
/// applies once the value is known present, so the two can never fail
/// together.
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct MatchParticipantRequest {
    /// Who piloted this side for the whole series. Free text, optional --
    /// there is no `player` table to validate it against.
    #[garde(skip)]
    #[serde(default)]
    pub player_label: Option<String>,
    /// Every hero this side drafted for the series, fielded or not. The side is
    /// this entry's position in `participants`. Must include every hero the
    /// side then played, or the submission is `PLAYED_HERO_NOT_DRAFTED`.
    #[garde(custom(max_entries(128, "draftedHeroIds must not exceed 128 entries")))]
    #[serde(default)]
    pub drafted_hero_ids: Vec<i64>,
}

#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct MatchGameParticipantRequest {
    #[garde(custom(required("heroId is required")))]
    pub hero_id: Option<i64>,
    /// No positivity rule: a losing hero finishing on negative health is legal
    /// -- the loser-must-be-at-or-below-zero rule lives in
    /// [`umfl_domain::match_policy`], which needs to *see* negative values to
    /// enforce it -- and the winner is unrestricted by the schema's own check.
    #[garde(custom(required("healthRemaining is required")))]
    pub health_remaining: Option<i32>,
    #[garde(skip)]
    #[serde(default)]
    pub is_winner: bool,
}

#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct MatchGameRequest {
    #[garde(custom(required_positive("gameNumber is required", "gameNumber must be positive")))]
    pub game_number: Option<i32>,
    #[garde(custom(required("mapId is required")))]
    pub map_id: Option<i64>,
    #[garde(dive, custom(exactly_two("exactly two sides are required")))]
    pub participants: Option<Vec<MatchGameParticipantRequest>>,
}

#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct MatchBanRequest {
    #[garde(custom(required("heroId is required")))]
    pub hero_id: Option<i64>,
    #[garde(custom(required("banType is required")))]
    pub ban_type: Option<BanType>,
    /// Whose draft this hero was struck out of, 0 or 1. Omit it for a
    /// `PRE_BAN` -- that happens before sides are assigned -- and the server
    /// answers 422 `BAN_SIDE_INVALID` if one is sent anyway. Omitting it on a
    /// typed ban is allowed but loses the attribution.
    #[garde(skip)]
    #[serde(default)]
    pub side: Option<i32>,
}

#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct RecordMatchRequest {
    #[garde(custom(required_positive("round is required", "round must be positive")))]
    pub round: Option<i32>,
    #[garde(custom(required("playedAt is required")))]
    #[serde(default, with = "umfl_domain::time::java_instant_opt")]
    pub played_at: Option<DateTime<Utc>>,
    /// Required and unique within the tournament: it is the duplicate check
    /// that stops the same match being imported twice. A match with no page
    /// anywhere still needs an identifier of its own.
    #[garde(custom(required_text("externalLink is required")))]
    pub external_link: Option<String>,
    #[garde(dive, custom(exactly_two("exactly two participants are required")))]
    pub participants: Option<Vec<MatchParticipantRequest>>,
    #[garde(
        dive,
        custom(between(1, 20, "games must contain between 1 and 20 entries"))
    )]
    pub games: Option<Vec<MatchGameRequest>>,
    #[garde(dive, custom(max_entries(64, "bans must not exceed 64 entries")))]
    #[serde(default)]
    pub bans: Vec<MatchBanRequest>,
}

/// Fails only on absent.
fn required<T>(message: &'static str) -> impl Fn(&Option<T>, &()) -> garde::Result {
    move |value, _| match value {
        Some(_) => Ok(()),
        None => Err(garde::Error::new(message)),
    }
}

/// Fails on absent *and* on whitespace-only.
fn required_text(message: &'static str) -> impl Fn(&Option<String>, &()) -> garde::Result {
    move |value, _| match value {
        Some(text) if !text.trim().is_empty() => Ok(()),
        _ => Err(garde::Error::new(message)),
    }
}

/// Present-and-positive on one field: absence and non-positivity are checked
/// separately, so exactly one of the two can fail.
fn required_positive(
    absent: &'static str,
    non_positive: &'static str,
) -> impl Fn(&Option<i32>, &()) -> garde::Result {
    move |value, _| match value {
        None => Err(garde::Error::new(absent)),
        Some(n) if *n <= 0 => Err(garde::Error::new(non_positive)),
        Some(_) => Ok(()),
    }
}

/// Rejects a present list of any length but 2; passes an absent one through
/// -- an absent list is read as empty by the caller, which is live behaviour
/// rather than an oversight.
fn exactly_two<T>(message: &'static str) -> impl Fn(&Option<Vec<T>>, &()) -> garde::Result {
    move |value, _| match value {
        Some(items) if items.len() != 2 => Err(garde::Error::new(message)),
        _ => Ok(()),
    }
}

/// Rejects a present list outside `[min, max]`; passes an absent one through.
fn between<T>(
    min: usize,
    max: usize,
    message: &'static str,
) -> impl Fn(&Option<Vec<T>>, &()) -> garde::Result {
    move |value, _| match value {
        Some(items) if items.len() < min || items.len() > max => Err(garde::Error::new(message)),
        _ => Ok(()),
    }
}

/// Rejects an oversized list. Takes a plain `Vec` rather than an `Option`,
/// since the field this guards defaults to empty rather than being optional.
fn max_entries<T>(max: usize, message: &'static str) -> impl Fn(&Vec<T>, &()) -> garde::Result {
    move |value, _| {
        if value.len() > max {
            Err(garde::Error::new(message))
        } else {
            Ok(())
        }
    }
}

impl RecordMatchRequest {
    /// The service's inputs, once validation has run -- which is why every
    /// `expect` below is unreachable.
    fn to_inputs(
        &self,
    ) -> (
        Vec<MatchParticipantInput>,
        Vec<MatchGameInput>,
        Vec<MatchBanInput>,
    ) {
        let participants = self
            .participants
            .iter()
            .flatten()
            .map(|p| MatchParticipantInput {
                player_label: p.player_label.clone(),
                drafted_hero_ids: p.drafted_hero_ids.clone(),
            })
            .collect();
        let games = self
            .games
            .iter()
            .flatten()
            .map(|g| MatchGameInput {
                game_number: g.game_number.expect("validated as present"),
                map_id: g.map_id.expect("validated as present"),
                participants: g
                    .participants
                    .iter()
                    .flatten()
                    .map(|p| MatchGameParticipantInput {
                        hero_id: p.hero_id.expect("validated as present"),
                        health_remaining: p.health_remaining.expect("validated as present"),
                        is_winner: p.is_winner,
                    })
                    .collect(),
            })
            .collect();
        let bans = self
            .bans
            .iter()
            .map(|b| MatchBanInput {
                hero_id: b.hero_id.expect("validated as present"),
                ban_type: b.ban_type.expect("validated as present"),
                side: b.side,
            })
            .collect();
        (participants, games, bans)
    }
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/tournaments/{tournament_id}/matches",
            get(list_matches).post(record),
        )
        .route(
            "/api/admin/tournaments/{tournament_id}/matches/{match_id}",
            get(get_match).put(correct).delete(delete),
        )
}

/// `round` and `limit` are both optional query parameters; `limit` defaults
/// to 200.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    #[serde(default)]
    round: Option<i32>,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    200
}

/// Recorded matches, newest first, optionally narrowed to one round.
///
/// Bounded: every match comes back with all of its games, game participants
/// and bans attached, so an unbounded list is the one admin read whose cost
/// grows with the tournament forever. `limit` defaults to 200 -- a page well
/// past any real tournament's match count -- and is clamped the same way the
/// ticker's is.
async fn list_matches(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(tournament_id): AppPath<i64>,
    AppQuery(params): AppQuery<ListQuery>,
) -> ApiResult<Json<Vec<MatchResultDto>>> {
    let matches = admin_service::list(
        &state,
        tournament_id,
        params.round,
        params.limit.clamp(1, 500),
    )
    .await?;
    Ok(Json(
        matches.into_iter().map(MatchResultDto::from).collect(),
    ))
}

async fn get_match(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, match_id)): AppPath<(i64, i64)>,
) -> ApiResult<Json<MatchResultDto>> {
    let result = admin_service::get(&state, tournament_id, match_id).await?;
    Ok(Json(MatchResultDto::from(result)))
}

async fn record(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(tournament_id): AppPath<i64>,
    ValidJson(request): ValidJson<RecordMatchRequest>,
) -> ApiResult<impl IntoResponse> {
    let (participants, games, bans) = request.to_inputs();
    let result = admin_service::record(
        &state,
        tournament_id,
        request.round.expect("validated as present"),
        request.played_at.expect("validated as present"),
        request
            .external_link
            .as_deref()
            .expect("validated as present"),
        &participants,
        &games,
        &bans,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(MatchResultDto::from(result))))
}

async fn correct(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, match_id)): AppPath<(i64, i64)>,
    ValidJson(request): ValidJson<RecordMatchRequest>,
) -> ApiResult<Json<MatchResultDto>> {
    let (participants, games, bans) = request.to_inputs();
    let result = admin_service::correct(
        &state,
        tournament_id,
        match_id,
        request.round.expect("validated as present"),
        request.played_at.expect("validated as present"),
        request
            .external_link
            .as_deref()
            .expect("validated as present"),
        &participants,
        &games,
        &bans,
    )
    .await?;
    Ok(Json(MatchResultDto::from(result)))
}

async fn delete(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, match_id)): AppPath<(i64, i64)>,
) -> ApiResult<StatusCode> {
    admin_service::delete(&state, tournament_id, match_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
