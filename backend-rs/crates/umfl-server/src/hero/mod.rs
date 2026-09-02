//! The hero pool that feeds the Roster Builder grid, and its admin half:
//! hero identities and per-tournament pricing.
//!
//! Oracle: `api/HeroController.kt`, `api/AdminHeroController.kt`, the hero
//! block of `api/AdminDtos.kt`, `hero/HeroQueryRepository.kt`, `hero/Hero.kt`,
//! `hero/HeroRepository.kt`, `hero/HeroPoolAdminRepository.kt` and
//! `hero/AdminHeroService.kt`.
//!
//! The public route lives *under* the tournament because cost is
//! tournament-scoped: a bare `/api/heroes` could not answer the Roster
//! Builder's actual question, which is "what can I pick here, and what does
//! it cost me". The admin routes are their own, separate endpoints (see
//! `AdminHeroController.listPool`'s doc) even where the shape happens to
//! match today.
//!
//! Deliberately thin — per-hero stat exploration is out of scope, and
//! third-party sites already publish Unmatched statistics. See AGENTS.md,
//! "Deliberately not built".

pub mod admin_service;
pub mod pool_admin;
pub mod query;
pub mod writer;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::CurrentManager;
use crate::error::ApiResult;
use crate::http::extract::{AppPath, AppQuery, ValidJson};
use crate::state::AppState;

pub use query::{HeroFilter, HeroSort, HeroView};

/// A hero's identity — name and artwork, nothing else. Cost is
/// tournament-scoped and lives in `tournament_heroes`, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hero {
    pub id: Option<i64>,
    pub name: String,
    pub image_url: Option<String>,
}

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

/// A hero's identity, with no cost — the catalogue as the admin dashboard's
/// Hero Management wizard sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeroAdminDto {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

impl From<Hero> for HeroAdminDto {
    fn from(hero: Hero) -> Self {
        Self {
            id: hero.id.expect("a saved hero has an id"),
            name: hero.name,
            image_url: hero.image_url,
        }
    }
}

/// `@NotBlank(message = "name is required")`.
///
/// A `custom` rule rather than garde's built-in, because the message is what
/// the client renders and garde 0.23 cannot override a built-in rule's
/// wording.
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateHeroRequest {
    #[garde(custom(required_text("name is required")))]
    pub name: Option<String>,
    #[serde(default)]
    #[garde(skip)]
    pub image_url: Option<String>,
}

/// `typealias UpdateHeroRequest = CreateHeroRequest` -- same body, same rule.
pub type UpdateHeroRequest = CreateHeroRequest;

/// `@Positive(message = "cost must be positive")`, with no `@NotNull` beside
/// it in the Kotlin -- the same shape as `capacity`/`rosterSize`/`creditGrant`
/// in `AdminTournamentController` (PORTING.md deviation (a)), and fixed the
/// same way: an absent `cost` is a 400 naming the field rather than a 500 from
/// the controller's `requireNotNull`.
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct SetHeroCostRequest {
    #[garde(custom(required_positive("must not be null", "cost must be positive")))]
    pub cost: Option<i32>,
}

/// One entry of a batch pool add: `@NotNull(message = "heroId is required")`
/// plus the same `@Positive`-without-`@NotNull` `cost` shape as
/// [`SetHeroCostRequest`].
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct HeroPoolEntryRequest {
    #[garde(custom(required("heroId is required")))]
    pub hero_id: Option<i64>,
    #[garde(custom(required_positive("must not be null", "cost must be positive")))]
    pub cost: Option<i32>,
}

/// `@Size(min = 1, max = 128, message = "heroes must contain between 1 and
/// 128 entries")`. Bean Validation's `@Size` ignores a null and there is no
/// `@NotNull` beside it, so an omitted `heroes` is valid and adds nothing --
/// the controller reads it as `request.heroes.orEmpty()`.
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct AddHeroesToPoolRequest {
    #[garde(dive, custom(batch_size))]
    pub heroes: Option<Vec<HeroPoolEntryRequest>>,
}

/// `@NotNull`.
fn required<T>(message: &'static str) -> impl Fn(&Option<T>, &()) -> garde::Result {
    move |value, _| match value {
        Some(_) => Ok(()),
        None => Err(garde::Error::new(message)),
    }
}

/// `@NotBlank`: fails on absent *and* on whitespace-only.
fn required_text(message: &'static str) -> impl Fn(&Option<String>, &()) -> garde::Result {
    move |value, _| match value {
        Some(text) if !text.trim().is_empty() => Ok(()),
        _ => Err(garde::Error::new(message)),
    }
}

/// `@NotNull` and `@Positive` on one field. `@Positive` ignores a null, so
/// exactly one of the two can fail -- see [`SetHeroCostRequest`]'s doc.
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

/// `@Size(min = 1, max = 128)`, ignoring a null as `@Size` does.
fn batch_size(value: &Option<Vec<HeroPoolEntryRequest>>, _: &()) -> garde::Result {
    match value {
        Some(entries) if entries.is_empty() || entries.len() > 128 => Err(garde::Error::new(
            "heroes must contain between 1 and 128 entries",
        )),
        _ => Ok(()),
    }
}

/// One entry of a batch pool add/re-price, once validation has run.
#[derive(Debug, Clone, Copy)]
pub struct HeroPoolEntryInput {
    pub hero_id: i64,
    pub cost: i32,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tournaments/{tournament_id}/heroes", get(list))
        .route("/api/admin/heroes", get(admin_list).post(create))
        .route("/api/admin/heroes/{id}", put(update))
        .route(
            "/api/admin/tournaments/{tournament_id}/heroes",
            get(list_pool).post(add_batch_to_pool),
        )
        .route(
            "/api/admin/tournaments/{tournament_id}/heroes/{hero_id}",
            put(set_pool_cost).delete(remove_from_pool),
        )
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

// `hasRole('ADMIN')` is enforced by `auth::authorize` for every `/api/admin/**`
// path -- the `@PreAuthorize` on the controller and the URL matcher in one
// place. Each handler still takes the `CurrentManager` its Kotlin counterpart
// declares, so the identity a route needs stays visible at the route.

async fn admin_list(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
) -> ApiResult<Json<Vec<HeroAdminDto>>> {
    let heroes = admin_service::list(&state).await?;
    Ok(Json(heroes.into_iter().map(HeroAdminDto::from).collect()))
}

/// The admin-scoped view of a tournament's hero pool, distinct from the
/// public `GET /api/tournaments/{id}/heroes` the Roster Builder reads -- the
/// two happen to return the same shape today but are separate endpoints so an
/// admin-only filter can be added later without reshaping the player-facing
/// one. Compare `AdminMapController.listPool`.
async fn list_pool(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(tournament_id): AppPath<i64>,
) -> ApiResult<Json<Vec<HeroDto>>> {
    let heroes = admin_service::pool(&state, tournament_id).await?;
    Ok(Json(heroes.into_iter().map(HeroDto::from).collect()))
}

async fn create(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    ValidJson(request): ValidJson<CreateHeroRequest>,
) -> ApiResult<impl IntoResponse> {
    let name = request.name.expect("validated as present");
    let hero = admin_service::create(&state, &name, request.image_url.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(HeroAdminDto::from(hero))))
}

async fn update(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(id): AppPath<i64>,
    ValidJson(request): ValidJson<UpdateHeroRequest>,
) -> ApiResult<Json<HeroAdminDto>> {
    let name = request.name.expect("validated as present");
    let hero = admin_service::update(&state, id, &name, request.image_url.as_deref()).await?;
    Ok(Json(HeroAdminDto::from(hero)))
}

/// Batch counterpart to [`set_pool_cost`] -- adds/re-prices several heroes in
/// one request.
async fn add_batch_to_pool(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(tournament_id): AppPath<i64>,
    ValidJson(request): ValidJson<AddHeroesToPoolRequest>,
) -> ApiResult<Json<Vec<HeroDto>>> {
    let entries: Vec<HeroPoolEntryInput> = request
        .heroes
        .unwrap_or_default()
        .into_iter()
        .map(|e| HeroPoolEntryInput {
            hero_id: e.hero_id.expect("validated as present"),
            cost: e.cost.expect("validated as present"),
        })
        .collect();
    let heroes = admin_service::add_batch_to_pool(&state, tournament_id, &entries).await?;
    Ok(Json(heroes.into_iter().map(HeroDto::from).collect()))
}

async fn set_pool_cost(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, hero_id)): AppPath<(i64, i64)>,
    ValidJson(request): ValidJson<SetHeroCostRequest>,
) -> ApiResult<Json<HeroDto>> {
    let cost = request.cost.expect("validated as present");
    let view = admin_service::set_pool_cost(&state, tournament_id, hero_id, cost).await?;
    Ok(Json(HeroDto::from(view)))
}

async fn remove_from_pool(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, hero_id)): AppPath<(i64, i64)>,
) -> ApiResult<StatusCode> {
    admin_service::remove_from_pool(&state, tournament_id, hero_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
