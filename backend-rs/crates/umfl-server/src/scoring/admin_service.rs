//! Create, update, list and activate a tournament's scoring rule sets.
//!
//! Oracle: `scoring/AdminScoringService.kt`. Every `@Transactional` method
//! there is a `pool.begin()` here and nothing else is (PORTING.md §7).
//!
//! The rules themselves are not here: they are
//! [`umfl_domain::scoring_rule_set_policy`], which validates the *shape* of a
//! metric name and never the *set* of legal names. A metric no extractor
//! implements is a non-blocking warning on the response
//! ([`umfl_domain::match_metrics::unknown`]), never a rejection, and must stay
//! one -- `ScoringRuleSetWizard.vue` renders those warnings after every save,
//! which is the only thing standing between a typo and a column that silently
//! scores zero forever.

use sqlx::PgConnection;
use umfl_domain::match_metrics;
use umfl_domain::scoring_rule_set_policy::{self, ScoringCoefficientInput};
use umfl_domain::{DomainError, Violation};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::tournament::service::require_tournament;

use super::{ScoringCoefficient, ScoringRuleSet, query, writer};

/// One rule set as create/update/list return it, paired with any metrics
/// nothing prices.
#[derive(Debug, Clone)]
pub struct ScoringRuleSetResult {
    pub rule_set: ScoringRuleSet,
    pub unknown_metrics: Vec<String>,
}

/// Every rule set for `tournament_id`, active one first -- an admin's only way
/// to see what already exists.
///
/// Transactional despite being a read, exactly as the Kotlin's
/// `@Transactional(readOnly = true)` is: the roots and their coefficients are
/// separate statements, so without one the listing could straddle a concurrent
/// activate or update.
pub async fn list(state: &AppState, tournament_id: i64) -> ApiResult<Vec<ScoringRuleSetResult>> {
    let mut tx = state.pool.begin().await?;
    require_tournament(&mut *tx, tournament_id).await?;
    let mut rule_sets = query::find_by_tournament_id(&mut tx, tournament_id).await?;
    tx.commit().await?;

    // `compareByDescending { it.isActive }.thenBy { it.name }`. `sort_by`, not
    // `sort_unstable_by` -- PORTING.md §8.
    rule_sets.sort_by(|a, b| {
        b.is_active
            .cmp(&a.is_active)
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(rule_sets.into_iter().map(with_warnings).collect())
}

pub async fn create(
    state: &AppState,
    tournament_id: i64,
    name: &str,
    coefficients: &[ScoringCoefficientInput],
    activate_now: bool,
) -> ApiResult<ScoringRuleSetResult> {
    let mut tx = state.pool.begin().await?;

    require_tournament(&mut *tx, tournament_id).await?;
    validate(coefficients)?;
    if query::find_by_tournament_id_and_name(&mut tx, tournament_id, name)
        .await?
        .is_some()
    {
        return Err(name_taken(tournament_id, name).into());
    }

    let mut rule_set = ScoringRuleSet {
        id: None,
        tournament_id,
        name: name.to_owned(),
        is_active: false,
        coefficients: to_coefficients(coefficients),
    };
    rule_set.id = Some(writer::insert_rule_set(&mut tx, &rule_set).await?);

    if activate_now {
        rule_set = flip_active(&mut tx, rule_set).await?;
    }
    tx.commit().await?;

    // The warnings come off the *submitted* metrics, not the stored ones -- the
    // same list either way, since `unknown` normalises what it is given.
    Ok(ScoringRuleSetResult {
        rule_set,
        unknown_metrics: unknown_metrics(coefficients),
    })
}

pub async fn update(
    state: &AppState,
    tournament_id: i64,
    rule_set_id: i64,
    name: &str,
    coefficients: &[ScoringCoefficientInput],
) -> ApiResult<ScoringRuleSetResult> {
    let mut tx = state.pool.begin().await?;

    let existing = require_rule_set(&mut tx, tournament_id, rule_set_id).await?;
    validate(coefficients)?;
    let collision = query::find_by_tournament_id_and_name(&mut tx, tournament_id, name).await?;
    if collision.is_some_and(|other| other.id != Some(rule_set_id)) {
        return Err(name_taken(tournament_id, name).into());
    }

    // `existing.copy(name = ..., coefficients = ...)`: `is_active` is carried
    // over untouched, so renaming the active rule set does not deactivate it.
    let rule_set = ScoringRuleSet {
        name: name.to_owned(),
        coefficients: to_coefficients(coefficients),
        ..existing
    };
    writer::update_rule_set(&mut tx, &rule_set).await?;
    tx.commit().await?;

    Ok(ScoringRuleSetResult {
        rule_set,
        unknown_metrics: unknown_metrics(coefficients),
    })
}

/// Activates `rule_set_id`, once it is confirmed to belong to `tournament_id`.
pub async fn activate(
    state: &AppState,
    tournament_id: i64,
    rule_set_id: i64,
) -> ApiResult<ScoringRuleSet> {
    let mut tx = state.pool.begin().await?;
    let target = require_rule_set(&mut tx, tournament_id, rule_set_id).await?;
    let activated = flip_active(&mut tx, target).await?;
    tx.commit().await?;
    Ok(activated)
}

/// The flag flip itself, for a rule set already in hand -- [`create`] has just
/// written the row it wants activated, so re-reading it by id would only buy
/// back what it already holds.
///
/// Deactivates any currently-active sibling **first**, as two separate
/// statements, so the partial unique index never sees two active rows for the
/// same tournament at once. Neither goes through an aggregate write: flipping
/// the flag must not rewrite either rule set's coefficient rows.
async fn flip_active(conn: &mut PgConnection, target: ScoringRuleSet) -> ApiResult<ScoringRuleSet> {
    let rule_set_id = target.id.expect("cannot activate an unsaved rule set");

    writer::deactivate_others(conn, target.tournament_id, rule_set_id).await?;
    writer::activate(conn, rule_set_id).await?;

    Ok(ScoringRuleSet {
        is_active: true,
        ..target
    })
}

async fn require_rule_set(
    conn: &mut PgConnection,
    tournament_id: i64,
    rule_set_id: i64,
) -> ApiResult<ScoringRuleSet> {
    query::find_by_id(&mut *conn, rule_set_id)
        .await?
        .filter(|rule_set| rule_set.tournament_id == tournament_id)
        .ok_or_else(|| {
            DomainError::not_found(format!(
                "No scoring rule set {rule_set_id} for tournament {tournament_id}"
            ))
            .into()
        })
}

/// Duplicate and malformed metrics are caught here rather than left to the
/// `unique (rule_set_id, metric)` constraint and the format CHECK, which would
/// surface as the generic 409 backstop with nothing naming the bad row.
fn validate(coefficients: &[ScoringCoefficientInput]) -> ApiResult<()> {
    let violations = scoring_rule_set_policy::validate(coefficients);
    if violations.is_empty() {
        return Ok(());
    }
    Err(ApiError::Domain(DomainError::ScoringRule(
        violations.into_iter().map(Violation::from).collect(),
    )))
}

fn name_taken(tournament_id: i64, name: &str) -> DomainError {
    DomainError::conflict(format!(
        "A scoring rule set named '{name}' already exists for tournament {tournament_id}."
    ))
}

/// The stored form of what the admin submitted.
///
/// The metric is normalised on the way in, because that is the column value the
/// duplicate check was made against. The Kotlin collects into a `Set`, which
/// would drop an exactly-repeated row; nothing reaches here to drop, since
/// [`validate`] has already rejected a repeated metric.
fn to_coefficients(coefficients: &[ScoringCoefficientInput]) -> Vec<ScoringCoefficient> {
    coefficients
        .iter()
        .map(|c| ScoringCoefficient {
            id: None,
            metric: match_metrics::normalise(&c.metric),
            coefficient: c.coefficient,
            sort_order: c.sort_order,
        })
        .collect()
}

fn unknown_metrics(coefficients: &[ScoringCoefficientInput]) -> Vec<String> {
    match_metrics::unknown(coefficients.iter().map(|c| c.metric.as_str()))
}

fn with_warnings(rule_set: ScoringRuleSet) -> ScoringRuleSetResult {
    let unknown_metrics =
        match_metrics::unknown(rule_set.coefficients.iter().map(|c| c.metric.as_str()));
    ScoringRuleSetResult {
        rule_set,
        unknown_metrics,
    }
}
