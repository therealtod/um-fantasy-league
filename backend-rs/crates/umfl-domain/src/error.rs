//! The domain's error vocabulary.
//!
//! Each variant maps to exactly one HTTP status in `umfl-server`'s
//! `ApiError`; the mapping lives there so this crate stays free of anything
//! web-shaped.

use std::fmt;

/// One broken rule, as it appears on the wire.
///
/// The `rule` field is the *name* of the rule enum constant (`"BUDGET_EXCEEDED"`,
/// `"NOT_EXACTLY_ONE_WINNER"`, ...), which is what `frontend/src/api/client.ts`
/// reads back off `ApiError.violations`.
///
/// This is deliberately *not* generic over the three rule enums. Each policy
/// module owns its own enum and converts here, so adding a rule never touches
/// this file -- which is what lets the roster, match and scoring policies
/// evolve independently of each other.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Violation {
    pub rule: String,
    pub message: String,
}

impl Violation {
    pub fn new(rule: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            rule: rule.into(),
            message: message.into(),
        }
    }
}

/// Joins violation messages with `"; "` to build the exception message that
/// becomes the problem document's `detail`.
fn join(violations: &[Violation]) -> String {
    violations
        .iter()
        .map(|v| v.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

/// The three rule families are separate variants rather than one: they render
/// the same 422 shape but carry different titles and problem-type slugs, and
/// each vocabulary evolves independently.
#[derive(Debug, Clone)]
pub enum DomainError {
    NotFound(String),
    Conflict(String),
    ServiceUnavailable(String),
    RosterRule(Vec<Violation>),
    MatchRule(Vec<Violation>),
    ScoringRule(Vec<Violation>),
}

impl DomainError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::ServiceUnavailable(message.into())
    }

    /// The violations carried by a 422, or an empty slice for the other variants.
    pub fn violations(&self) -> &[Violation] {
        match self {
            Self::RosterRule(v) | Self::MatchRule(v) | Self::ScoringRule(v) => v,
            _ => &[],
        }
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(m) | Self::Conflict(m) | Self::ServiceUnavailable(m) => f.write_str(m),
            Self::RosterRule(v) | Self::MatchRule(v) | Self::ScoringRule(v) => {
                f.write_str(&join(v))
            }
        }
    }
}

impl std::error::Error for DomainError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_violation_messages_with_semicolons() {
        // Joined with `"; "`, which becomes the problem document's `detail`.
        let err = DomainError::RosterRule(vec![
            Violation::new(
                "INCOMPLETE_ROSTER",
                "Roster needs 3 heroes but only 2 selected.",
            ),
            Violation::new(
                "BUDGET_EXCEEDED",
                "Roster costs 10400 credits, exceeding the 10000 grant by 400.",
            ),
        ]);
        assert_eq!(
            err.to_string(),
            "Roster needs 3 heroes but only 2 selected.; \
             Roster costs 10400 credits, exceeding the 10000 grant by 400."
        );
    }

    #[test]
    fn non_rule_errors_carry_no_violations() {
        assert!(DomainError::not_found("nope").violations().is_empty());
    }
}
