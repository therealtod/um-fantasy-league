//! A tournament's scoring configuration: the rule sets an admin writes, which
//! the leaderboard is folded against.
//!
//! Oracle: `api/AdminScoringController.kt`, the scoring block of
//! `api/AdminDtos.kt`, and `scoring/AdminScoringService.kt`.
//!
//! Only the admin write side lives here. The arithmetic that *reads* a rule set
//! is already pure and already ported -- [`umfl_domain::scoring_engine`],
//! [`umfl_domain::match_metrics`] and
//! [`umfl_domain::scoring_rule_set_policy`] -- and the standings fold calls it
//! through [`query::active_rules`].
//!
//! Points are never stored. `scoring_coefficient` is mutable reference data
//! retuned with a bare `UPDATE`, so a stored total would be a cache with
//! nothing to invalidate it (AGENTS.md, "Nothing writes points"). Everything
//! written here is an *input* to a fold that happens at read time.

pub mod admin_service;
pub mod query;
pub mod writer;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use umfl_domain::scoring_rule_set_policy::ScoringCoefficientInput;

use crate::auth::CurrentManager;
use crate::error::ApiResult;
use crate::http::extract::{AppPath, ValidJson};
use crate::state::AppState;

use admin_service::ScoringRuleSetResult;

/// A tournament's scoring configuration -- the admin write side of what
/// [`query::active_rules`] reads.
///
/// At most one rule set may be active per tournament, enforced by the partial
/// unique index `uq_scoring_rule_set_active`, so activating one is a
/// service-level operation ([`admin_service::activate`]) rather than a field
/// flip.
///
/// The aggregate lives here rather than in `umfl-domain` because no pure rule
/// needs it: [`umfl_domain::scoring_rule_set_policy`] validates a slice of
/// [`ScoringCoefficientInput`], which it already owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoringRuleSet {
    pub id: Option<i64>,
    pub tournament_id: i64,
    pub name: String,
    pub is_active: bool,
    pub coefficients: Vec<ScoringCoefficient>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoringCoefficient {
    pub id: Option<i64>,
    pub metric: String,
    pub coefficient: Decimal,
    /// Fixes the leaderboard's left-to-right column order -- app data, not a
    /// list index the persistence layer maintains.
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoringRuleSetDto {
    pub id: i64,
    pub tournament_id: i64,
    pub name: String,
    pub is_active: bool,
    pub coefficients: Vec<ScoringCoefficientDto>,
    /// Metrics no extractor implements -- not rejected, just surfaced (e.g. the
    /// seed's `CROWD_FAVOURITE`). `ScoringRuleSetWizard.vue` renders these
    /// after every save.
    pub warnings: Vec<String>,
}

impl ScoringRuleSetDto {
    fn from_parts(rule_set: ScoringRuleSet, warnings: Vec<String>) -> Self {
        let mut coefficients: Vec<ScoringCoefficientDto> = rule_set
            .coefficients
            .into_iter()
            .map(ScoringCoefficientDto::from)
            .collect();
        // `sortedBy { it.sortOrder }`, and stable, so equal `sort_order`s keep
        // the order the query returned them in (PORTING.md §8).
        coefficients.sort_by_key(|c| c.sort_order);

        Self {
            id: rule_set.id.expect("a saved rule set has an id"),
            tournament_id: rule_set.tournament_id,
            name: rule_set.name,
            is_active: rule_set.is_active,
            coefficients,
            warnings,
        }
    }
}

impl From<ScoringRuleSetResult> for ScoringRuleSetDto {
    fn from(result: ScoringRuleSetResult) -> Self {
        Self::from_parts(result.rule_set, result.unknown_metrics)
    }
}

/// `coefficient` is a `numeric(10,4)` and goes out as a JSON *number* with its
/// scale intact -- see [`crate::http::big_decimal`], which is the only way to
/// keep `0.7500` from being flattened to `0.75`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoringCoefficientDto {
    pub metric: String,
    #[serde(with = "crate::http::big_decimal")]
    pub coefficient: Decimal,
    pub sort_order: i32,
}

impl From<ScoringCoefficient> for ScoringCoefficientDto {
    fn from(c: ScoringCoefficient) -> Self {
        Self {
            metric: c.metric,
            coefficient: c.coefficient,
            sort_order: c.sort_order,
        }
    }
}

/// `@NotBlank(message = "metric is required")` and
/// `@NotNull(message = "coefficient is required")`.
///
/// Both are garde `custom` rules rather than built-ins, because the message is
/// what the client renders and garde 0.23 cannot override a built-in rule's
/// wording.
#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct ScoringCoefficientRequest {
    #[garde(custom(required_text("metric is required")))]
    pub metric: Option<String>,
    #[garde(custom(required("coefficient is required")))]
    #[serde(default, with = "crate::http::big_decimal::option")]
    pub coefficient: Option<Decimal>,
    #[garde(skip)]
    #[serde(default)]
    pub sort_order: i32,
}

impl ScoringCoefficientRequest {
    /// Validation has already run, so both `requireNotNull`s the controller
    /// makes are unreachable here for the same reason they are there.
    fn to_input(&self) -> ScoringCoefficientInput {
        ScoringCoefficientInput {
            metric: self.metric.clone().expect("validated as present"),
            coefficient: self.coefficient.expect("validated as present"),
            sort_order: self.sort_order,
        }
    }
}

#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateScoringRuleSetRequest {
    #[garde(custom(required_text("name is required")))]
    pub name: Option<String>,
    #[garde(dive, custom(at_least_one_coefficient))]
    pub coefficients: Option<Vec<ScoringCoefficientRequest>>,
    /// Activate this rule set immediately, deactivating any current sibling.
    #[garde(skip)]
    #[serde(default)]
    pub activate: bool,
}

#[derive(Debug, Deserialize, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScoringRuleSetRequest {
    #[garde(custom(required_text("name is required")))]
    pub name: Option<String>,
    #[garde(dive, custom(at_least_one_coefficient))]
    pub coefficients: Option<Vec<ScoringCoefficientRequest>>,
}

/// `@NotBlank`: fails on absent *and* on whitespace-only.
fn required_text(message: &'static str) -> impl Fn(&Option<String>, &()) -> garde::Result {
    move |value, _| match value {
        Some(text) if !text.trim().is_empty() => Ok(()),
        _ => Err(garde::Error::new(message)),
    }
}

/// `@NotNull`.
fn required<T>(message: &'static str) -> impl Fn(&Option<T>, &()) -> garde::Result {
    move |value, _| match value {
        Some(_) => Ok(()),
        None => Err(garde::Error::new(message)),
    }
}

/// `@Size(min = 1, message = "at least one coefficient is required")`.
///
/// Bean Validation's `@Size` **ignores a null**, and there is no `@NotNull`
/// beside it, so a request that omits `coefficients` entirely is valid and
/// creates a rule set with none. That is a live behaviour of the Kotlin -- the
/// controller reads it as `request.coefficients.orEmpty()` -- and porting it
/// faithfully means the absent case passes here too.
fn at_least_one_coefficient(
    value: &Option<Vec<ScoringCoefficientRequest>>,
    _: &(),
) -> garde::Result {
    match value {
        Some(coefficients) if coefficients.is_empty() => {
            Err(garde::Error::new("at least one coefficient is required"))
        }
        _ => Ok(()),
    }
}

fn to_inputs(
    coefficients: &Option<Vec<ScoringCoefficientRequest>>,
) -> Vec<ScoringCoefficientInput> {
    coefficients
        .iter()
        .flatten()
        .map(ScoringCoefficientRequest::to_input)
        .collect()
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/tournaments/{tournament_id}/scoring-rule-sets",
            get(list).post(create),
        )
        .route(
            "/api/admin/tournaments/{tournament_id}/scoring-rule-sets/{rule_set_id}",
            put(update),
        )
        .route(
            "/api/admin/tournaments/{tournament_id}/scoring-rule-sets/{rule_set_id}/activate",
            post(activate),
        )
}

// `hasRole('ADMIN')` is enforced by `auth::authorize` for every `/api/admin/**`
// path, which is the `@PreAuthorize` on the controller *and* the URL matcher in
// one place. Each handler still takes the `CurrentManager` its Kotlin
// counterpart declares, so the identity a route needs is visible at the route.

async fn list(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(tournament_id): AppPath<i64>,
) -> ApiResult<Json<Vec<ScoringRuleSetDto>>> {
    let rule_sets = admin_service::list(&state, tournament_id).await?;
    Ok(Json(
        rule_sets.into_iter().map(ScoringRuleSetDto::from).collect(),
    ))
}

async fn create(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath(tournament_id): AppPath<i64>,
    ValidJson(request): ValidJson<CreateScoringRuleSetRequest>,
) -> ApiResult<impl IntoResponse> {
    let result = admin_service::create(
        &state,
        tournament_id,
        &request.name.clone().expect("validated as present"),
        &to_inputs(&request.coefficients),
        request.activate,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(ScoringRuleSetDto::from(result))))
}

async fn update(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, rule_set_id)): AppPath<(i64, i64)>,
    ValidJson(request): ValidJson<UpdateScoringRuleSetRequest>,
) -> ApiResult<Json<ScoringRuleSetDto>> {
    let result = admin_service::update(
        &state,
        tournament_id,
        rule_set_id,
        &request.name.clone().expect("validated as present"),
        &to_inputs(&request.coefficients),
    )
    .await?;
    Ok(Json(ScoringRuleSetDto::from(result)))
}

/// Activation carries **no** warnings: the Kotlin builds this response with
/// `ScoringRuleSetDto.from(ruleSet)`, whose `warnings` parameter defaults to
/// empty. Recomputing them here would be an extra field on the wire.
async fn activate(
    State(state): State<AppState>,
    CurrentManager(_admin): CurrentManager,
    AppPath((tournament_id, rule_set_id)): AppPath<(i64, i64)>,
) -> ApiResult<Json<ScoringRuleSetDto>> {
    let rule_set = admin_service::activate(&state, tournament_id, rule_set_id).await?;
    Ok(Json(ScoringRuleSetDto::from_parts(rule_set, Vec::new())))
}
