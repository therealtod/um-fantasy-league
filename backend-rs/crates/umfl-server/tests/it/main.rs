//! Every integration test in the crate, in **one** binary.
//!
//! Cargo compiles each `tests/*.rs` into its own binary with its own process,
//! and this harness starts a PostgreSQL container per process. Splitting the
//! suite across files at the top level would therefore start a container each
//! -- so the suite is one binary (`[[test]] name = "it"` in `Cargo.toml`) and
//! every test file is a `mod` of it. Adding a test area means adding a `mod`
//! line here and a file next to this one; it never means a new `tests/*.rs`.
//!
//! Layer 2 of PORTING.md §13. Layer 1 -- the pure domain tests -- lives in
//! `umfl-domain` and needs neither Docker nor this file.

mod harness;

mod actuator;
mod roster_flow;
mod schema_and_seed;
mod scoring_admin;
mod security;
mod tournament_api;
