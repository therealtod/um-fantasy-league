//! The hero pool that feeds the Roster Builder grid.
//!
//! Oracle: `api/HeroController.kt`, `hero/HeroQueryRepository.kt`, `hero/Hero.kt`.
//!
//! The route lives *under* the tournament because cost is tournament-scoped: a
//! bare `/api/heroes` could not answer the Roster Builder's actual question,
//! which is "what can I pick here, and what does it cost me".
//!
//! Deliberately thin — per-hero stat exploration is out of scope, and
//! third-party sites already publish Unmatched statistics. See AGENTS.md,
//! "Deliberately not built".

pub mod query;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::http::extract::{AppPath, AppQuery};
use crate::state::AppState;

pub use query::{HeroFilter, HeroSort, HeroView};

/// A hero and what it costs in the tournament that was asked about.
///
/// `image_url` is nullable in `heroes` and therefore **absent** rather than
/// `null` — `frontend/src/api/types.ts` types it `string | null` and every
/// template that shows one carries a `?? '—'` fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeroDto {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    pub cost: i32,
}

impl From<HeroView> for HeroDto {
    fn from(view: HeroView) -> Self {
        Self {
            id: view.id,
            name: view.name,
            image_url: view.image_url,
            cost: view.cost,
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/tournaments/{tournament_id}/heroes", get(list))
}

/// `@RequestParam(required = false) search` and
/// `@RequestParam(required = false, defaultValue = "COST") sort`.
#[derive(Debug, Default, Deserialize)]
struct ListQuery {
    search: Option<String>,
    #[serde(default)]
    sort: HeroSort,
}

async fn list(
    State(state): State<AppState>,
    AppPath(tournament_id): AppPath<i64>,
    AppQuery(params): AppQuery<ListQuery>,
) -> ApiResult<Json<Vec<HeroDto>>> {
    // 404 on an unknown tournament rather than a silently empty pool.
    crate::tournament::service::require_tournament(&state.pool, tournament_id).await?;

    let filter = HeroFilter {
        search: params.search,
        sort: params.sort,
    };
    let heroes = query::find_by_tournament(&state.pool, tournament_id, &filter).await?;
    Ok(Json(heroes.into_iter().map(HeroDto::from).collect()))
}
