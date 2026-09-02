//! Boards, and which of them a tournament may be played on.
//!
//! Oracle: `api/AdminMapController.kt`, the map block of `api/AdminDtos.kt`,
//! `map/GameMap.kt`, `map/GameMapRepository.kt`,
//! `map/MapPoolAdminRepository.kt` and `map/AdminMapService.kt`.
//!
//! The catalogue is reference data -- facts about Unmatched, seeded by
//! `V2__reference_data.sql` and extended when Restoration Games releases a
//! board. The *pool* is league data: which of those boards this tournament
//! actually plays on, and the thing `match_games` is constrained against.
//!
//! Both halves are admin-only. There is no public board route, because nothing
//! outside the admin dashboard asks for one -- a match names its board inline.

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
use crate::http::extract::{AppPath, ValidJson};
use crate::state::AppState;

/// A board a match can be played on.
///
/// Legality per tournament lives in `tournament_maps`, not here: the same board
/// can be in one tournament's pool and out of another's, which is why this
/// carries no tournament at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMap {
    pub id: Option<i64>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapAdminDto {
    pub id: i64,
    pub name: String,
}

impl From<GameMap> for MapAdminDto {
    fn from(map: GameMap) -> Self {
        Self {
            id: map.id.expect("a saved map has an id"),
            name: map.name,
        }
    }
}

/// `@NotBlank(message = "name is required")`.
///
/// A `custom` rule rather than garde's built-in, because the message is what
/// the client renders and garde 0.23 cannot override a built-in rule's wording.
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateMapRequest {
    #[garde(custom(required_text("name is required")))]
    pub name: Option<String>,
}

/// `typealias UpdateMapRequest = CreateMapRequest` -- same body, same rule.
pub type UpdateMapRequest = CreateMapRequest;

#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct AddMapsToPoolRequest {
    #[garde(custom(batch_size))]
    pub map_ids: Option<Vec<i64>>,
}

/// `@NotBlank`: fails on absent *and* on whitespace-only.
fn required_text(message: &'static str) -> impl Fn(&Option<String>, &()) -> garde::Result {
    move |value, _| match value {
        Some(text) if !text.trim().is_empty() => Ok(()),
        _ => Err(garde::Error::new(message)),
    }
}

/// `@Size(min = 1, max = 64, message = "mapIds must contain between 1 and 64 entries")`.
///
/// Bean Validation's `@Size` **ignores a null** and there is no `@NotNull`
/// beside it, so a request that omits `mapIds` entirely is valid and adds
/// nothing -- the controller reads it as `request.mapIds.orEmpty()`. Porting
/// faithfully means the absent case passes here too.
fn batch_size(value: &Option<Vec<i64>>, _: &()) -> garde::Result {
    match value {
        Some(ids) if ids.is_empty() || ids.len() > 64 => Err(garde::Error::new(
            "mapIds must contain between 1 and 64 entries",
        )),
        _ => Ok(()),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/maps", get(list).post(create))
        .route("/api/admin/maps/{id}", put(update))
        .route(
            "/api/admin/tournaments/{tournament_id}/maps",
            get(list_pool).post(add_batch_to_pool),
        )
        .route(
            "/api/admin/tournaments/{tournament_id}/maps/{map_id}",
            put(add_to_pool).delete(remove_from_pool),
        )
}

// `hasRole('ADMIN')` is enforced by `auth::authorize` for every `/api/admin/**`
// path -- the `@PreAuthorize` on the controller and the URL matcher in one
// place. Each handler still takes the `CurrentManager` its Kotlin counterpart
// declares, so the identity a route needs stays visible at the route.

async fn list(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
) -> ApiResult<Json<Vec<MapAdminDto>>> {
    let maps = admin_service::list(&state).await?;
    Ok(Json(maps.into_iter().map(MapAdminDto::from).collect()))
}

async fn list_pool(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(tournament_id): AppPath<i64>,
) -> ApiResult<Json<Vec<MapAdminDto>>> {
    let maps = admin_service::list_pool(&state, tournament_id).await?;
    Ok(Json(maps.into_iter().map(MapAdminDto::from).collect()))
}

async fn create(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    ValidJson(request): ValidJson<CreateMapRequest>,
) -> ApiResult<impl IntoResponse> {
    let name = request.name.expect("validated as present");
    let map = admin_service::create(&state, &name).await?;
    Ok((StatusCode::CREATED, Json(MapAdminDto::from(map))))
}

async fn update(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(id): AppPath<i64>,
    ValidJson(request): ValidJson<UpdateMapRequest>,
) -> ApiResult<Json<MapAdminDto>> {
    let name = request.name.expect("validated as present");
    let map = admin_service::update(&state, id, &name).await?;
    Ok(Json(MapAdminDto::from(map)))
}

/// The batch add. **200, not 201** -- `AdminMapController.addBatchToPool`
/// carries no `@ResponseStatus`, unlike `create` above.
async fn add_batch_to_pool(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(tournament_id): AppPath<i64>,
    ValidJson(request): ValidJson<AddMapsToPoolRequest>,
) -> ApiResult<Json<Vec<MapAdminDto>>> {
    let map_ids = request.map_ids.unwrap_or_default();
    let maps = admin_service::add_batch_to_pool(&state, tournament_id, &map_ids).await?;
    Ok(Json(maps.into_iter().map(MapAdminDto::from).collect()))
}

async fn add_to_pool(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, map_id)): AppPath<(i64, i64)>,
) -> ApiResult<Json<MapAdminDto>> {
    let map = admin_service::add_to_pool(&state, tournament_id, map_id).await?;
    Ok(Json(MapAdminDto::from(map)))
}

async fn remove_from_pool(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, map_id)): AppPath<(i64, i64)>,
) -> ApiResult<StatusCode> {
    admin_service::remove_from_pool(&state, tournament_id, map_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
