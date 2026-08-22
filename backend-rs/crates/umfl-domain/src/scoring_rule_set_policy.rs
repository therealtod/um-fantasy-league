//! Pre-validates a scoring rule set's coefficients before the service saves
//! them, so a bad admin submission comes back as a named 422 instead of the
//! generic 409 a `unique (rule_set_id, metric)` or format-CHECK failure would
//! produce further down.
//!
//! A direct port of `scoring/ScoringRuleSetPolicy.kt`.
//!
//! It validates the *shape* of a metric name, never the *set* of legal names: a
//! metric no extractor implements is a deliberate non-blocking warning
//! ([`crate::match_metrics::unknown`]), not a rejection, and must stay one.

use crate::Violation;
use crate::match_metrics;
use indexmap::IndexMap;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScoringRule {
    /// Two coefficients in the same rule set price the same metric.
    DuplicateMetric,

    /// A metric name is not SCREAMING_SNAKE_CASE, so the schema's format CHECK
    /// would reject it.
    MalformedMetric,
}

impl ScoringRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateMetric => "DUPLICATE_METRIC",
            Self::MalformedMetric => "MALFORMED_METRIC",
        }
    }
}

impl std::fmt::Display for ScoringRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoringViolation {
    pub rule: ScoringRule,
    pub message: String,
}

impl ScoringViolation {
    fn new(rule: ScoringRule, message: impl Into<String>) -> Self {
        Self {
            rule,
            message: message.into(),
        }
    }
}

impl From<ScoringViolation> for Violation {
    fn from(v: ScoringViolation) -> Self {
        Violation::new(v.rule.as_str(), v.message)
    }
}

/// One priced metric as the admin submitted it -- before normalisation, which
/// is the whole point: `' win '` and `'Win'` arrive distinct and must be caught.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoringCoefficientInput {
    pub metric: String,
    pub coefficient: Decimal,
    pub sort_order: i32,
}

/// Mirrors the `scoring_coefficient_metric_format` CHECK in
/// `V1__core_schema.sql`: `^[A-Z][A-Z0-9_]*$`.
///
/// Hand-written rather than a regex because `umfl-domain` has no regex
/// dependency and this is four lines. The Kotlin uses `Regex.matches`, which
/// anchors both ends against the whole input -- so a trailing newline fails
/// there too, and this loop reproduces that without the `$`-before-newline
/// subtlety a partial-match API would introduce.
fn is_well_formed(metric: &str) -> bool {
    let mut chars = metric.chars();
    match chars.next() {
        Some(first) if first.is_ascii_uppercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Both checks run against the *normalised* metric, because that is what gets
/// stored -- `' win '` and `'Win'` are the same column, so they are also a
/// duplicate of each other.
///
/// Every broken rule is reported, not just the first.
pub fn validate(coefficients: &[ScoringCoefficientInput]) -> Vec<ScoringViolation> {
    let mut violations = Vec::new();

    let metrics: Vec<String> = coefficients
        .iter()
        .map(|c| match_metrics::normalise(&c.metric))
        .collect();

    // `filterNot(METRIC_FORMAT::matches).distinct()` -- encounter order, which
    // is the order the message lists them in.
    let mut malformed: Vec<&str> = Vec::new();
    for metric in &metrics {
        if !is_well_formed(metric) && !malformed.contains(&metric.as_str()) {
            malformed.push(metric);
        }
    }
    if !malformed.is_empty() {
        let listed = malformed
            .iter()
            .map(|m| format!("'{m}'"))
            .collect::<Vec<_>>()
            .join(", ");
        violations.push(ScoringViolation::new(
            ScoringRule::MalformedMetric,
            format!(
                "Metric name(s) must be letters, digits and underscores starting with a letter: \
                 {listed}."
            ),
        ));
    }

    // `groupingBy { it }.eachCount().filterValues { it > 1 }.keys`, then
    // `.sorted()`.
    let mut counts: IndexMap<&str, usize> = IndexMap::new();
    for metric in &metrics {
        *counts.entry(metric.as_str()).or_default() += 1;
    }
    let mut duplicates: Vec<&str> = counts
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(metric, _)| metric)
        .collect();
    duplicates.sort();

    if !duplicates.is_empty() {
        let listed = duplicates
            .iter()
            .map(|m| format!("'{m}'"))
            .collect::<Vec<_>>()
            .join(", ");
        violations.push(ScoringViolation::new(
            ScoringRule::DuplicateMetric,
            format!("Metric(s) priced more than once: {listed}."),
        ));
    }

    violations
}

/// A near-1:1 port of `ScoringRuleSetPolicyTest`.
#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn coefficient(metric: &str) -> ScoringCoefficientInput {
        weighted(metric, "1.0", 0)
    }

    fn weighted(metric: &str, weight: &str, sort_order: i32) -> ScoringCoefficientInput {
        ScoringCoefficientInput {
            metric: metric.into(),
            coefficient: Decimal::from_str(weight).unwrap(),
            sort_order,
        }
    }

    fn rules(violations: &[ScoringViolation]) -> Vec<ScoringRule> {
        violations.iter().map(|v| v.rule).collect()
    }

    #[test]
    fn a_well_formed_rule_set_has_no_violations() {
        let violations = validate(&[
            weighted("WIN", "3.0", 0),
            weighted("HEALTH_REMAINING", "0.5", 1),
        ]);

        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    #[test]
    fn the_same_metric_priced_twice_is_rejected() {
        let violations = validate(&[weighted("WIN", "1.0", 0), weighted("WIN", "2.0", 0)]);

        assert_eq!(rules(&violations), vec![ScoringRule::DuplicateMetric]);
        assert_eq!(
            violations[0].message,
            "Metric(s) priced more than once: 'WIN'."
        );
    }

    /// `normalise` folds these to the same stored column, so they are
    /// duplicates of each other.
    #[test]
    fn metrics_that_differ_only_in_case_or_padding_are_duplicates() {
        let violations = validate(&[coefficient(" win "), coefficient("Win")]);

        assert_eq!(rules(&violations), vec![ScoringRule::DuplicateMetric]);
    }

    #[test]
    fn a_hyphenated_metric_is_rejected_rather_than_reaching_the_schemas_format_check() {
        let violations = validate(&[coefficient("win-rate")]);

        assert_eq!(rules(&violations), vec![ScoringRule::MalformedMetric]);
        assert_eq!(
            violations[0].message,
            "Metric name(s) must be letters, digits and underscores starting with a letter: \
             'WIN-RATE'."
        );
    }

    #[test]
    fn a_metric_starting_with_a_digit_is_rejected() {
        let violations = validate(&[coefficient("1st")]);

        assert_eq!(rules(&violations), vec![ScoringRule::MalformedMetric]);
    }

    #[test]
    fn a_blank_metric_is_rejected() {
        let violations = validate(&[coefficient("   ")]);

        assert_eq!(rules(&violations), vec![ScoringRule::MalformedMetric]);
    }

    #[test]
    fn digits_and_underscores_after_the_first_letter_are_legal() {
        let violations = validate(&[coefficient("BEST_OF_3")]);

        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    /// The registry deliberately ignores metrics it can't price -- that is a
    /// warning on the response, never a rejection. This is the guard against
    /// someone turning [`validate`] into a whitelist.
    #[test]
    fn a_metric_no_extractor_implements_is_legal() {
        let violations = validate(&[coefficient("CROWD_FAVOURITE")]);

        assert!(violations.is_empty(), "expected none, got {violations:?}");
        assert_eq!(
            match_metrics::unknown(["CROWD_FAVOURITE"]),
            vec!["CROWD_FAVOURITE".to_string()]
        );
    }

    /// A negative weight is a penalty, which the schema deliberately allows.
    #[test]
    fn a_negative_coefficient_is_legal() {
        let violations = validate(&[weighted("LOSS", "-1.5", 0)]);

        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    #[test]
    fn every_broken_rule_is_reported_not_just_the_first() {
        let violations = validate(&[
            coefficient("win-rate"),
            coefficient("WIN"),
            coefficient("win"),
        ]);

        assert_eq!(
            rules(&violations),
            vec![ScoringRule::MalformedMetric, ScoringRule::DuplicateMetric]
        );
    }

    #[test]
    fn violations_convert_to_the_wire_shape() {
        let wire: Vec<Violation> = validate(&[coefficient("1st")])
            .into_iter()
            .map(Into::into)
            .collect();

        assert_eq!(wire[0].rule, "MALFORMED_METRIC");
    }
}
