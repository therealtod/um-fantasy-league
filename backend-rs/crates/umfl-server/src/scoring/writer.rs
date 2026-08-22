//! Scoring rule-set writes.
//!
//! Oracle: `scoring/ScoringRuleSet.kt`'s `@MappedCollection` (via
//! `ScoringRuleSetRepository.save`) and `scoring/ScoringRuleSetAdminRepository.kt`.
//!
//! Spring Data JDBC's `save` on a root owning a child collection inserts the
//! root then its children, or updates the root and **deletes and reinserts**
//! every child -- it does not diff. [`insert_rule_set`] and
//! [`update_rule_set`] reproduce exactly that, for the same reason
//! `tournament::writer` does: an in-place child update would have to reconcile
//! `unique (rule_set_id, metric)` mid-statement, where a wholesale replace
//! never meets it.
//!
//! # Why the two `is_active` statements are not an aggregate save
//!
//! The Kotlin keeps [`deactivate_others`] and [`activate`] in a separate
//! `@Repository` (`ScoringRuleSetAdminRepository`) precisely so activation
//! cannot go through `save`: flipping one boolean through the aggregate would
//! churn the whole coefficient table -- on *both* the outgoing and the incoming
//! rule set -- and burn a sequence value per row.
//!
//! Here they are ordinary functions in the file the naming convention reserves
//! for writes, and the hazard they were separated to avoid does not exist:
//! [`update_rule_set`] is hand-written, so nothing cascades by accident. The
//! reason they stay separate *functions* is unchanged and still load-bearing --
//! `AdminScoringServiceIntegrationTest` asserts that activating leaves both
//! rule sets' coefficient ids untouched.
//!
//! Every function takes the connection: each is more than one statement, and
//! all of them belong to somebody's transaction (PORTING.md §7).

use sqlx::PgConnection;

use super::{ScoringCoefficient, ScoringRuleSet};

/// Inserts a rule set and its coefficients, returning the generated root id.
pub async fn insert_rule_set(
    conn: &mut PgConnection,
    rule_set: &ScoringRuleSet,
) -> sqlx::Result<i64> {
    let id = sqlx::query_scalar!(
        "insert into scoring_rule_set (tournament_id, name, is_active)
         values ($1, $2, $3) returning id",
        rule_set.tournament_id,
        rule_set.name,
        rule_set.is_active
    )
    .fetch_one(&mut *conn)
    .await?;

    insert_coefficients(conn, id, &rule_set.coefficients).await?;
    Ok(id)
}

/// Updates an existing rule set: the root row, then its coefficients wholesale.
///
/// # Panics
///
/// On a rule set with no id. Unreachable -- the only caller loaded it from
/// [`super::query::find_by_id`] -- and it is the `requireNotNull` the Kotlin
/// service makes at the same point.
pub async fn update_rule_set(
    conn: &mut PgConnection,
    rule_set: &ScoringRuleSet,
) -> sqlx::Result<()> {
    let id = rule_set.id.expect("a loaded rule set has an id");

    sqlx::query!(
        "update scoring_rule_set
            set tournament_id = $2, name = $3, is_active = $4
          where id = $1",
        id,
        rule_set.tournament_id,
        rule_set.name,
        rule_set.is_active
    )
    .execute(&mut *conn)
    .await?;

    sqlx::query!("delete from scoring_coefficient where rule_set_id = $1", id)
        .execute(&mut *conn)
        .await?;
    insert_coefficients(conn, id, &rule_set.coefficients).await?;
    Ok(())
}

/// Clears `is_active` on every rule set of `tournament_id` except
/// `except_rule_set_id`.
///
/// Must run **before** [`activate`] so the partial unique index
/// `uq_scoring_rule_set_active` never sees two active rows at once.
pub async fn deactivate_others(
    conn: &mut PgConnection,
    tournament_id: i64,
    except_rule_set_id: i64,
) -> sqlx::Result<()> {
    sqlx::query!(
        "update scoring_rule_set
            set is_active = false
          where tournament_id = $1
            and id <> $2
            and is_active",
        tournament_id,
        except_rule_set_id
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn activate(conn: &mut PgConnection, rule_set_id: i64) -> sqlx::Result<()> {
    sqlx::query!(
        "update scoring_rule_set set is_active = true where id = $1",
        rule_set_id
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// One statement for the whole collection, with each row's values taken from
/// the arrays by position -- so nothing here can drift out of step with the
/// slice the caller passed.
async fn insert_coefficients(
    conn: &mut PgConnection,
    rule_set_id: i64,
    coefficients: &[ScoringCoefficient],
) -> sqlx::Result<()> {
    if coefficients.is_empty() {
        return Ok(());
    }
    let metrics: Vec<String> = coefficients.iter().map(|c| c.metric.clone()).collect();
    let weights: Vec<_> = coefficients.iter().map(|c| c.coefficient).collect();
    let sort_orders: Vec<i32> = coefficients.iter().map(|c| c.sort_order).collect();

    sqlx::query!(
        "insert into scoring_coefficient (rule_set_id, metric, coefficient, sort_order)
         select $1, m, w, s
         from unnest($2::text[], $3::numeric[], $4::integer[]) as t(m, w, s)",
        rule_set_id,
        &metrics,
        &weights,
        &sort_orders
    )
    .execute(conn)
    .await?;
    Ok(())
}
