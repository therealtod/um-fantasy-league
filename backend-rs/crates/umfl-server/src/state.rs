//! The handle every handler is given: shared state passed explicitly rather
//! than injected.
//!
//! Feature tasks **append** fields here (the match-result cache, the SSE hub,
//! the JWKS cache). That makes this one of the two files with a real merge
//! surface -- the other is `api/mod.rs` -- so keep additions to one field and
//! one line of construction, and put the type itself in the feature's own
//! module.

use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool, Postgres, Transaction};

use crate::auth::supabase::JwksCache;
use crate::config::Config;
use crate::r#match::MatchResultCache;
use crate::matchimport::{HttpScraperClient, ScraperClient};
use crate::ratelimit::RateLimiter;
use crate::standings::StandingsSseHub;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    /// The `/api/**` throttle's bucket store. Built once and shared, because a
    /// per-request limiter would have no memory to limit with.
    pub rate_limiter: RateLimiter,
    /// Supabase's signing keys, fetched lazily and only in `prod`. Empty and
    /// untouched in every other profile, whose credential is a header.
    pub jwks: JwksCache,
    /// The assembled match list per tournament, held between writes. Shared
    /// for the same reason `rate_limiter` is: a per-request cache would have
    /// no memory to cache with.
    pub match_cache: MatchResultCache,
    /// The open standings streams, keyed by tournament. Shared for the same
    /// reason `match_cache` is: a per-request hub would have nobody to notify.
    pub standings_hub: StandingsSseHub,
    /// The scraper sidecar, behind this crate's **one** trait. It is a trait
    /// object rather than a concrete client because it is a genuine test
    /// seam: `tests/it/match_import.rs`'s `StubScraper` substitutes a stub
    /// rather than standing up a real browser.
    pub scraper: Arc<dyn ScraperClient>,
}

impl AppState {
    pub async fn connect(config: Config) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            // Explicit, and not a number to tune casually: ten is HikariCP's
            // default and therefore the figure `MatchResultCache` was designed
            // against -- one match write can tell up to 200 tabs per tournament
            // to refetch, and each pulls both the board and the ticker head.
            .max_connections(config.max_connections)
            .connect(&config.database_url)
            .await?;
        let rate_limiter = RateLimiter::new(&config.rate_limit);
        let jwks = JwksCache::new(config.supabase_jwks_uri.clone());
        let match_cache = MatchResultCache::new();
        let standings_hub = StandingsSseHub::new();
        let scraper: Arc<dyn ScraperClient> =
            Arc::new(HttpScraperClient::new(config.scraper_base_url.clone()));
        Ok(Self {
            pool,
            config: Arc::new(config),
            rate_limiter,
            jwks,
            match_cache,
            standings_hub,
            scraper,
        })
    }
}

/// `BEGIN`, then the isolation level as the **very next statement**.
///
/// Postgres accepts `set transaction isolation level` only before the
/// transaction's first query; issued any later it is not an error, it is a
/// silent no-op that leaves the transaction at READ COMMITTED.
/// A multi-statement read that quietly degrades that way still returns
/// plausible rows -- just rows from several different snapshots -- so nothing
/// surfaces until someone doubts the leaderboard. That is exactly the failure
/// mode a named helper exists to make unrepeatable.
///
/// Read-only as well as REPEATABLE READ, and the pairing matters: Postgres
/// raises a serialization failure only on a write/write conflict, so a
/// read-only transaction at this level needs no retry handling at all.
///
/// It lives here rather than in either caller because there are now two --
/// `standings::service`'s board/ticker snapshot and `match::cache`'s loader --
/// and they are in different features. One of them holding the definition
/// would make the other import a transaction boundary from a module it has no
/// other business with; duplicating three lines of SQL would leave the rule
/// stated twice and fixable once. `state` is what both already depend on.
pub async fn read_snapshot(pool: &PgPool) -> sqlx::Result<Transaction<'static, Postgres>> {
    let mut tx = pool.begin().await?;
    tx.execute("set transaction isolation level repeatable read read only")
        .await?;
    Ok(tx)
}
