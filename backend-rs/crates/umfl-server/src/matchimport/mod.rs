//! Admin-only: turn a Tabletop League match URL into a reviewable draft.
//!
//! It sits beside `crate::r#match` rather than inside it because it does a
//! categorically different thing: **this endpoint writes nothing.** It scrapes,
//! resolves the source's hero and board names onto this league's rows, and
//! returns a draft. Recording it is a separate, deliberate POST to the ordinary
//! record endpoint, which is what makes an imported match go through
//! `umfl_domain::match_policy` exactly as a hand-typed one does.
//!
//! The preview types are one set of structs, not a domain type plus a
//! separate DTO: the preview is a response shape, not a domain rule, so
//! there is nothing a second copy would protect. `frontend/src/api/types.ts`
//! is the JSON contract.

pub mod query;
pub mod scraper;
pub mod service;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use umfl_domain::match_result::BanType;

use crate::auth::CurrentManager;
use crate::error::ApiResult;
use crate::http::extract::{AppPath, ValidJson};
use crate::state::AppState;

pub use scraper::{HttpScraperClient, ScraperClient};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/admin/tournaments/{tournament_id}/matches/import",
        post(import),
    )
}

/// Answers **200 with unresolved names listed** rather than failing on them: a
/// board missing from the pool or a hero missing from the catalogue is
/// something the admin fixes and re-imports, and the rest of the scrape is
/// still worth showing them. The genuinely broken cases -- a URL that is not a
/// match page, a scraper that cannot read the page, a scraper that is not
/// running -- surface as 409 or 503.
async fn import(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(tournament_id): AppPath<i64>,
    ValidJson(request): ValidJson<ImportMatchRequest>,
) -> ApiResult<Json<MatchImportPreview>> {
    let source_url = request.source_url.unwrap_or_default();
    Ok(Json(
        service::preview(&state, tournament_id, &source_url).await?,
    ))
}

#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct ImportMatchRequest {
    #[garde(custom(required_text("sourceUrl is required")))]
    pub source_url: Option<String>,
}

/// Fails on absent *and* on whitespace-only.
fn required_text(message: &'static str) -> impl Fn(&Option<String>, &()) -> garde::Result {
    move |value, _| match value {
        Some(text) if !text.trim().is_empty() => Ok(()),
        _ => Err(garde::Error::new(message)),
    }
}

/// A scraped match resolved against one tournament's catalogue, ready for an
/// admin to review -- not a recorded match.
///
/// Two fields are absent by design rather than by failure: there is no `round`,
/// because the source names its pools ("The Wayward Sisters") where the schema
/// wants a positive `i32`, and `played_at` when the source's timestamp carried
/// a timezone abbreviation that could not be resolved. Both are for the admin
/// to fill in, and `round_name` / `played_at_raw` are carried through so they
/// have something to go on.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchImportPreview {
    pub source_url: String,
    /// The source's own name for the round -- context for the admin, never
    /// stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_name: Option<String>,
    /// e.g. `"BO3"` -- context only; nothing in this domain records a series
    /// format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_format: Option<String>,
    #[serde(
        with = "umfl_domain::time::java_instant_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub played_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub played_at_raw: Option<String>,
    /// Set when this tournament already has a match whose `external_link` is
    /// [`Self::source_url`] -- which `uq_tournament_match_external_link` makes
    /// at most one.
    ///
    /// A block, not a warning: recording it again would double-count everything
    /// the match scores, and `match::admin_service` refuses the write. The
    /// panel shows this so the admin is sent to correct that match rather than
    /// discovering the conflict after filling the wizard in. Correcting an
    /// imported match by re-importing it stays legitimate -- that path updates
    /// the existing row instead of creating a second one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub already_imported_match_id: Option<i64>,
    pub participants: Vec<ImportedParticipant>,
    pub games: Vec<ImportedGame>,
    pub bans: Vec<ImportedBan>,
    /// Every name that could not be resolved. Non-empty means the draft is
    /// incomplete.
    pub unresolved: Vec<UnresolvedName>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedParticipant {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_label: Option<String>,
    /// Every hero this side drafted, fielded or not -- the full list, matching
    /// `MatchParticipantRequest.draftedHeroIds`. Note the admin *form* holds
    /// only the unfielded subset and re-unions at save time, so the frontend
    /// subtracts before seeding the wizard.
    pub drafted_hero_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedGame {
    pub game_number: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map_name: Option<String>,
    pub participants: Vec<ImportedGameParticipant>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedGameParticipant {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hero_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hero_name: Option<String>,
    pub health_remaining: i32,
    pub is_winner: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedBan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hero_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hero_name: Option<String>,
    pub ban_type: BanType,
    /// Whose draft this hero was struck out of, or absent for a `PRE_BAN`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<i32>,
}

/// What kind of row a name failed to resolve to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UnresolvedKind {
    Hero,
    Map,
}

/// Why a name could not be used.
///
/// [`UnresolvedReason::MapNotInPool`] is the one that actually fires in
/// practice: `match_games` carries a composite foreign key onto
/// `tournament_maps`, so a board this league knows about but has not added to
/// *this tournament's* pool cannot be recorded against it. Heroes have no such
/// constraint -- `match_game_participants.hero_id` and `hero_bans.hero_id`
/// reference `heroes(id)` directly, never `tournament_heroes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UnresolvedReason {
    UnknownHero,
    UnknownMap,
    MapNotInPool,
}

/// Hashed and compared by every field, because the service collects these in a
/// set: one hero missing from the catalogue can appear as a pick, a game
/// participant and a ban, and the admin should be told once.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedName {
    pub kind: UnresolvedKind,
    /// The name exactly as the source site rendered it, so the admin can
    /// recognise it.
    pub source_name: String,
    pub reason: UnresolvedReason,
    /// Set for [`UnresolvedReason::MapNotInPool`] -- the board exists, it just
    /// is not in the pool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map_id: Option<i64>,
    pub message: String,
}
