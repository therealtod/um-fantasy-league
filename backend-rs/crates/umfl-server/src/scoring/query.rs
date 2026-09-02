//! Scoring rule-set reads.
//!
//! Oracle: `scoring/ScoringRuleSetQuery.kt` (the `JdbcClient` projection) and
//! the derived queries on `scoring/ScoringRuleSetRepository.kt`.
//!
//! Both halves are reads and both live here, which is the split the Kotlin
//! class names were carrying (PORTING.md §3); `ScoringRuleSetRepository.save`
//! is `writer.rs`.
//!
//! # Loading the aggregate
//!
//! Spring Data JDBC loads a root's `@MappedCollection` children in a statement
//! of their own, one per root. These functions issue **one** child statement
//! for every root instead. The rows are identical and both callers are inside
//! a transaction, so the two cannot observe a different database; what changes
//! is the statement count, which is why `AdminScoringService.list` was
//! transactional in the first place.

use indexmap::IndexMap;
use rust_decimal::Decimal;
use sqlx::{PgConnection, PgExecutor};
use umfl_domain::match_metrics;
use umfl_domain::scoring_engine::ScoringRules;

use super::{ScoringCoefficient, ScoringRuleSet};

/// The tournament's active scoring configuration, or [`ScoringRules::none`].
///
/// At most one rule set per tournament can be active -- enforced by the partial
/// unique index `uq_scoring_rule_set_active`, not by this query. A tournament
/// with none scores zero, which is a legitimate state (a freshly created
/// event), so this returns the empty rules rather than failing.
///
/// The `left join` is what makes a rule set with no coefficients yet still
/// yield its id and name, and is why the Kotlin's row type has a nullable
/// `metric`: `sqlx` types that column `Option<String>` for the same reason, and
/// the `else { continue }` below is the Kotlin's `?: continue`.
pub async fn active_rules(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
) -> sqlx::Result<ScoringRules> {
    let rows = sqlx::query!(
        // `as "metric?"` / `as "coefficient?"`: the columns are `not null` in
        // their own table, so sqlx infers them non-nullable and cannot see
        // that the `left join` makes them optional here. The Kotlin row type
        // marks the same two fields nullable for the same reason.
        r#"select rs.id as rule_set_id, rs.name, c.metric as "metric?", c.coefficient as "coefficient?"
           from scoring_rule_sets rs
           left join scoring_coefficients c on c.rule_set_id = rs.id
           where rs.tournament_id = $1
             and rs.is_active
           order by c.sort_order, c.metric"#,
        tournament_id
    )
    .fetch_all(db)
    .await?;

    let Some(header) = rows.first() else {
        return Ok(ScoringRules::none());
    };
    let (rule_set_id, name) = (header.rule_set_id, header.name.clone());

    // An `IndexMap` keeps `sort_order`, which is the leaderboard's column
    // order -- the Kotlin's `linkedMapOf` (PORTING.md §4.2).
    let mut coefficients: IndexMap<String, Decimal> = IndexMap::with_capacity(rows.len());
    for row in rows {
        let Some(metric) = row.metric else { continue };
        coefficients.insert(
            match_metrics::normalise(&metric),
            row.coefficient.unwrap_or(Decimal::ZERO),
        );
    }
    Ok(ScoringRules::new(rule_set_id, name, coefficients))
}

/// `findByTournamentId`, coefficients included.
pub async fn find_by_tournament_id(
    conn: &mut PgConnection,
    tournament_id: i64,
) -> sqlx::Result<Vec<ScoringRuleSet>> {
    let roots = sqlx::query!(
        "select id, tournament_id, name, is_active
         from scoring_rule_sets where tournament_id = $1 order by id",
        tournament_id
    )
    .fetch_all(&mut *conn)
    .await?;

    let ids: Vec<i64> = roots.iter().map(|r| r.id).collect();
    let mut children = coefficients_by_rule_set(conn, &ids).await?;

    Ok(roots
        .into_iter()
        .map(|r| ScoringRuleSet {
            id: Some(r.id),
            tournament_id: r.tournament_id,
            name: r.name,
            is_active: r.is_active,
            coefficients: children.swap_remove(&r.id).unwrap_or_default(),
        })
        .collect())
}

/// `findByTournamentIdAndName` -- the unique `(tournament_id, name)` lookup
/// both `create` and `update` use to name a collision before the index does.
pub async fn find_by_tournament_id_and_name(
    conn: &mut PgConnection,
    tournament_id: i64,
    name: &str,
) -> sqlx::Result<Option<ScoringRuleSet>> {
    let Some(root) = sqlx::query!(
        "select id, tournament_id, name, is_active
         from scoring_rule_sets where tournament_id = $1 and name = $2",
        tournament_id,
        name
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(ScoringRuleSet {
        id: Some(root.id),
        tournament_id: root.tournament_id,
        name: root.name,
        is_active: root.is_active,
        coefficients: coefficients_of(conn, root.id).await?,
    }))
}

/// `findById`. The tournament filter the Kotlin applies afterwards is the
/// service's, not this function's.
pub async fn find_by_id(
    conn: &mut PgConnection,
    rule_set_id: i64,
) -> sqlx::Result<Option<ScoringRuleSet>> {
    let Some(root) = sqlx::query!(
        "select id, tournament_id, name, is_active from scoring_rule_sets where id = $1",
        rule_set_id
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(ScoringRuleSet {
        id: Some(root.id),
        tournament_id: root.tournament_id,
        name: root.name,
        is_active: root.is_active,
        coefficients: coefficients_of(conn, root.id).await?,
    }))
}

/// One rule set's coefficients, in column order.
///
/// `sort_order, id` rather than the Kotlin's nothing-in-particular: the child
/// collection is a `Set<ScoringCoefficient>` there, so its iteration order is
/// unspecified and `ScoringRuleSetDto.from` re-sorts by `sort_order` anyway.
/// Ordering here makes the tie deterministic instead of hash-dependent, and
/// `id` is insertion order, which is the order the admin submitted them in.
async fn coefficients_of(
    conn: &mut PgConnection,
    rule_set_id: i64,
) -> sqlx::Result<Vec<ScoringCoefficient>> {
    let rows = sqlx::query!(
        "select id, metric, coefficient, sort_order
         from scoring_coefficients where rule_set_id = $1 order by sort_order, id",
        rule_set_id
    )
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ScoringCoefficient {
            id: Some(r.id),
            metric: r.metric,
            coefficient: r.coefficient,
            sort_order: r.sort_order,
        })
        .collect())
}

/// The same, for several rule sets at once, keyed by owner.
async fn coefficients_by_rule_set(
    conn: &mut PgConnection,
    rule_set_ids: &[i64],
) -> sqlx::Result<IndexMap<i64, Vec<ScoringCoefficient>>> {
    let mut by_rule_set: IndexMap<i64, Vec<ScoringCoefficient>> = IndexMap::new();
    if rule_set_ids.is_empty() {
        return Ok(by_rule_set);
    }
    let rows = sqlx::query!(
        "select rule_set_id, id, metric, coefficient, sort_order
         from scoring_coefficients where rule_set_id = any($1) order by sort_order, id",
        rule_set_ids
    )
    .fetch_all(conn)
    .await?;
    for r in rows {
        by_rule_set
            .entry(r.rule_set_id)
            .or_default()
            .push(ScoringCoefficient {
                id: Some(r.id),
                metric: r.metric,
                coefficient: r.coefficient,
                sort_order: r.sort_order,
            });
    }
    Ok(by_rule_set)
}
