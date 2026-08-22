//! The handle every handler is given, in place of Spring's injected beans.
//!
//! Feature tasks **append** fields here (the match-result cache, the SSE hub,
//! the JWKS cache). That makes this one of the two files with a real merge
//! surface -- the other is `api/mod.rs` -- so keep additions to one field and
//! one line of construction, and put the type itself in the feature's own
//! module.

use std::sync::Arc;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::auth::supabase::JwksCache;
use crate::config::Config;
use crate::ratelimit::RateLimiter;

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
        Ok(Self {
            pool,
            config: Arc::new(config),
            rate_limiter,
            jwks,
        })
    }
}
