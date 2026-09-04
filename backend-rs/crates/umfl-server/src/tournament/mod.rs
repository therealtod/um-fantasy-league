//! Tournaments, entries and the Roster Builder's write path.
//!
//! The DTOs live here rather than in one shared file; the JSON shape is
//! unchanged, and `frontend/src/api/types.ts` is the contract.

pub mod admin_service;
pub mod query;
pub mod service;
pub mod writer;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use umfl_domain::roster_policy::{self, BudgetStatus, RosterPick};
use umfl_domain::tournament::{EntryStatus, Tournament, TournamentFormat, TournamentStatus};

use crate::auth::{CurrentManager, MaybeManager};
use crate::error::ApiResult;
use crate::hero::HeroDto;
use crate::http::extract::{AppPath, AppQuery, ValidJson};
use crate::state::AppState;

use admin_service::TournamentFields;
use service::RosterSnapshot;

/// Dates serialise as plain `YYYY-MM-DD`: a tournament is announced as a day or
/// a weekend, not a wall-clock instant. `NaiveDate`'s serde is exactly that
/// format, so no helper is needed here the way `java_instant` is for `Instant`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentDto {
    pub id: i64,
    pub name: String,
    pub format: TournamentFormat,
    pub status: TournamentStatus,
    pub start_date: NaiveDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<NaiveDate>,
    pub capacity: i32,
    pub enrolled: i64,
    pub roster_size: i32,
    pub credit_grant: i32,
    pub accepts_registration: bool,
    /// Absent — not null — when the calling manager has not entered this
    /// tournament.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_entry_status: Option<EntryStatus>,
}

impl TournamentDto {
    fn from(tournament: Tournament, enrolled: i64, my_entry_status: Option<EntryStatus>) -> Self {
        Self {
            id: tournament.id.expect("a loaded tournament has an id"),
            name: tournament.name,
            format: tournament.format,
            status: tournament.status,
            start_date: tournament.start_date,
            end_date: tournament.end_date,
            capacity: tournament.capacity,
            enrolled,
            roster_size: tournament.roster_size,
            credit_grant: tournament.credit_grant,
            accepts_registration: matches!(tournament.status, TournamentStatus::RegistrationOpen),
            my_entry_status,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetStatusDto {
    pub spent: i32,
    pub credit_grant: i32,
    pub remaining: i32,
    pub utilisation: f64,
}

impl From<BudgetStatus> for BudgetStatusDto {
    fn from(status: BudgetStatus) -> Self {
        Self {
            spent: status.spent,
            credit_grant: status.credit_grant,
            remaining: status.remaining,
            utilisation: status.utilisation,
        }
    }
}

/// One manager's roster, as the Roster Builder renders it.
///
/// `locked_at` is skipped when absent even though `types.ts` declares it
/// `string | null`. That is **preserved** deliberately: `non_null` has been
/// omitting the field all along and the frontend copes; emitting `null`
/// would be the actual wire change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterDto {
    pub entry_id: i64,
    pub tournament_id: i64,
    pub tournament_name: String,
    pub status: EntryStatus,
    pub locked: bool,
    #[serde(
        with = "umfl_domain::time::java_instant_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub locked_at: Option<DateTime<Utc>>,
    pub roster_size: i32,
    pub heroes: Vec<HeroDto>,
    pub budget: BudgetStatusDto,
    /// True when `validate_lock` raises no violations for this roster — i.e.
    /// the lock button may enable.
    pub lockable: bool,
}

impl From<RosterSnapshot> for RosterDto {
    fn from(snapshot: RosterSnapshot) -> Self {
        let picks: Vec<RosterPick> = snapshot
            .heroes
            .iter()
            .map(|h| RosterPick {
                hero_id: h.id,
                cost: h.cost,
            })
            .collect();
        let lockable =
            roster_policy::validate_lock(&picks, &snapshot.tournament, &snapshot.entry).is_empty();

        Self {
            entry_id: snapshot.entry.id.expect("a saved entry has an id"),
            tournament_id: snapshot
                .tournament
                .id
                .expect("a loaded tournament has an id"),
            tournament_name: snapshot.tournament.name,
            status: snapshot.entry.status,
            locked: snapshot.entry.status == EntryStatus::Locked,
            locked_at: snapshot.entry.locked_at,
            roster_size: snapshot.tournament.roster_size,
            heroes: snapshot.heroes.into_iter().map(HeroDto::from).collect(),
            budget: snapshot.budget.into(),
            lockable,
        }
    }
}

/// `heroIds` must be present, and at most 64 entries.
///
/// Both checks are `custom` rather than garde's own `required` / `length`
/// rules, because the message string is what the client renders and garde
/// 0.23 has no way to override a built-in rule's wording. One function
/// covers both without ambiguity: the length check only applies once the
/// value is known present, so the two can never fail together.
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct SetSlotsRequest {
    #[garde(custom(hero_ids_rule))]
    pub hero_ids: Option<Vec<i64>>,
}

fn hero_ids_rule(value: &Option<Vec<i64>>, _: &()) -> garde::Result {
    match value {
        None => Err(garde::Error::new("heroIds is required")),
        Some(ids) if ids.len() > 64 => Err(garde::Error::new("heroIds must not exceed 64 entries")),
        Some(_) => Ok(()),
    }
}

/// `name` must be present and non-blank; every other field below must be
/// present, and `capacity`/`roster_size`/`credit_grant` must also be
/// positive -- an absent one is a 400 naming the field ("must not be null")
/// rather than a 500 from an unwrap deeper in the service.
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateTournamentRequest {
    #[garde(custom(required_text("name is required")))]
    pub name: Option<String>,
    #[garde(custom(required("format is required")))]
    pub format: Option<TournamentFormat>,
    #[garde(custom(required("status is required")))]
    pub status: Option<TournamentStatus>,
    #[garde(custom(required("startDate is required")))]
    pub start_date: Option<NaiveDate>,
    #[serde(default)]
    #[garde(skip)]
    pub end_date: Option<NaiveDate>,
    #[garde(custom(required_positive("must not be null", "capacity must be positive")))]
    pub capacity: Option<i32>,
    #[garde(custom(required_positive("must not be null", "rosterSize must be positive")))]
    pub roster_size: Option<i32>,
    #[garde(custom(required_positive("must not be null", "creditGrant must be positive")))]
    pub credit_grant: Option<i32>,
}

/// Full replace — every field is resubmitted, including `status`.
pub type UpdateTournamentRequest = CreateTournamentRequest;

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

impl CreateTournamentRequest {
    /// The service's inputs, once validation has run -- which is why every
    /// `expect` below is unreachable.
    fn to_fields(&self) -> TournamentFields<'_> {
        TournamentFields {
            name: self.name.as_deref().expect("validated as present"),
            format: self.format.expect("validated as present"),
            status: self.status.expect("validated as present"),
            start_date: self.start_date.expect("validated as present"),
            end_date: self.end_date,
            capacity: self.capacity.expect("validated as present"),
            roster_size: self.roster_size.expect("validated as present"),
            credit_grant: self.credit_grant.expect("validated as present"),
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tournaments", get(list))
        .route("/api/tournaments/{id}", get(get_one))
        .route("/api/tournaments/{id}/entries", post(register))
        .route("/api/tournaments/{id}/entries/me", get(my_entry))
        .route("/api/tournaments/{id}/entries/me/slots", put(set_slots))
        .route("/api/tournaments/{id}/entries/me/lock", post(lock))
        .route("/api/admin/tournaments", post(admin_create))
        .route(
            "/api/admin/tournaments/{id}",
            put(admin_update).delete(admin_delete),
        )
}

#[derive(Debug, Default, Deserialize)]
struct ListQuery {
    status: Option<TournamentStatus>,
}

async fn list(
    State(state): State<AppState>,
    MaybeManager(manager): MaybeManager,
    AppQuery(params): AppQuery<ListQuery>,
) -> ApiResult<Json<Vec<TournamentDto>>> {
    let enrolment = service::enrolment_counts(&state.pool).await?;
    // One query for every entry this manager holds, rather than one per card.
    let mine = match &manager {
        Some(m) => query::statuses_by_tournament(&state.pool, m.id).await?,
        None => Default::default(),
    };

    let tournaments = service::list_tournaments(&state.pool, params.status).await?;
    Ok(Json(
        tournaments
            .into_iter()
            .map(|t| {
                let id = t.id.expect("a loaded tournament has an id");
                TournamentDto::from(
                    t,
                    enrolment.get(&id).copied().unwrap_or(0),
                    mine.get(&id).copied(),
                )
            })
            .collect(),
    ))
}

async fn get_one(
    State(state): State<AppState>,
    MaybeManager(manager): MaybeManager,
    AppPath(id): AppPath<i64>,
) -> ApiResult<Json<TournamentDto>> {
    let tournament = service::require_tournament(&state.pool, id).await?;
    let enrolled = service::enrolment_count(&state.pool, id).await?;
    let my_entry_status = match &manager {
        Some(m) => query::status_for(&state.pool, m.id, id).await?,
        None => None,
    };
    Ok(Json(TournamentDto::from(
        tournament,
        enrolled,
        my_entry_status,
    )))
}

/// Enter this tournament: opens an empty draft roster with the tournament's
/// credit grant.
async fn register(
    State(state): State<AppState>,
    CurrentManager(manager): CurrentManager,
    AppPath(id): AppPath<i64>,
) -> ApiResult<impl IntoResponse> {
    let snapshot = service::register(&state, id, &manager).await?;
    Ok((StatusCode::CREATED, Json(RosterDto::from(snapshot))))
}

async fn my_entry(
    State(state): State<AppState>,
    CurrentManager(manager): CurrentManager,
    AppPath(id): AppPath<i64>,
) -> ApiResult<Json<RosterDto>> {
    service::find_my_entry(&state, id, &manager)
        .await?
        .map(|snapshot| Json(RosterDto::from(snapshot)))
        .ok_or_else(|| service::not_registered(id).into())
}

/// Replace the roster selection. Over-budget drafts are accepted — the budget is
/// enforced when locking — but duplicates, oversized rosters and heroes outside
/// this tournament's pool are rejected with 422.
async fn set_slots(
    State(state): State<AppState>,
    CurrentManager(manager): CurrentManager,
    AppPath(id): AppPath<i64>,
    ValidJson(request): ValidJson<SetSlotsRequest>,
) -> ApiResult<Json<RosterDto>> {
    let hero_ids = request.hero_ids.unwrap_or_default();
    let snapshot = service::set_slots(&state, id, &manager, &hero_ids).await?;
    Ok(Json(RosterDto::from(snapshot)))
}

/// Commit the roster. Requires a full roster within the entry's credit grant.
async fn lock(
    State(state): State<AppState>,
    CurrentManager(manager): CurrentManager,
    AppPath(id): AppPath<i64>,
) -> ApiResult<Json<RosterDto>> {
    let snapshot = service::lock_roster(&state, id, &manager).await?;
    Ok(Json(RosterDto::from(snapshot)))
}

// `Access::Admin` is enforced by `auth::authorize` for every `/api/admin/**`
// path -- the admission check and the URL matcher live in one place. Each
// handler still takes `CurrentManager` for the identity, so who a route
// needs stays visible at the route.

async fn admin_create(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    ValidJson(request): ValidJson<CreateTournamentRequest>,
) -> ApiResult<impl IntoResponse> {
    let tournament = admin_service::create(&state, request.to_fields()).await?;
    Ok((
        StatusCode::CREATED,
        Json(TournamentDto::from(tournament, 0, None)),
    ))
}

async fn admin_update(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(id): AppPath<i64>,
    ValidJson(request): ValidJson<UpdateTournamentRequest>,
) -> ApiResult<Json<TournamentDto>> {
    let tournament = admin_service::update(&state, id, request.to_fields()).await?;
    let enrolled = service::enrolment_count(&state.pool, id).await?;
    Ok(Json(TournamentDto::from(tournament, enrolled, None)))
}

async fn admin_delete(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(id): AppPath<i64>,
) -> ApiResult<StatusCode> {
    admin_service::delete(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
