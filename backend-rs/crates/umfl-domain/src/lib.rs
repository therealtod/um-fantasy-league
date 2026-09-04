//! The pure half of the UMFL backend: domain rules, scoring, and the wire-shape
//! helpers those rules depend on.
//!
//! This crate has no `sqlx`, no `axum`, no `tokio` and no `reqwest`, and must
//! not acquire them.

pub mod error;
pub mod match_result;
pub mod rounding;
pub mod time;

// Declared up front, by T0, so concurrent Tier-1 owners never race on this
// file. Each fills in exactly its own module and touches nothing here.
pub mod match_metrics;
pub mod match_policy;
pub mod name_resolver;
pub mod roster_policy;
pub mod scoring_engine;
pub mod scoring_rule_set_policy;
pub mod scraped_timestamps;
pub mod standings;
pub mod tournament;

pub use error::{DomainError, Violation};
