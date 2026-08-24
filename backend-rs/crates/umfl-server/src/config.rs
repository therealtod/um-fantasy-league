//! Deployment configuration, under the environment variable names the existing
//! deployments already set.
//!
//! Oracle: `application.yml`, `application-dev.yml`, `application-prod.yml`,
//! `RateLimitProperties.kt`, `ScraperProperties.kt`.
//!
//! Two rules from `AGENTS.md` shape this file:
//!
//! * **There are no `umfl.*` tunables.** Scoring weights are rows in
//!   `scoring_coefficient` and the budget is `tournament.credit_grant`; both
//!   are retuned with an UPDATE. The only configuration here is infrastructure
//!   -- the database, the port, the scraper's address, the throttle -- which is
//!   unreadable from a database the process has not reached yet.
//! * **The variable names do not change.** `SPRING_PROFILES_ACTIVE` is still
//!   `SPRING_PROFILES_ACTIVE`, spelt that way in both compose files and in the
//!   VPS's hand-managed `/opt/umfl/.env`. Renaming it would buy nothing and
//!   cost a manual edit on a box that CI deliberately never writes to.
//!
//! `DB_URL` likewise stays **JDBC-shaped**. Flyway still consumes it verbatim
//! as the migration mechanism, so only this process needs the conversion --
//! which is what keeps the credential change surface on the VPS at zero.

use std::time::Duration;

use figment::Figment;
use figment::providers::{Env, Serialized};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};

/// RFC 3986 `unreserved`. Everything else in a username or password is escaped
/// before it goes into the URL's userinfo, so a password containing `@`, `:` or
/// `/` cannot silently reshape the connection string.
const USERINFO: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[derive(Clone)]
pub struct Config {
    /// `SPRING_PROFILES_ACTIVE`, split on commas. Empty means the default
    /// profile: no seed data, prod-shaped security.
    pub profiles: Vec<String>,
    pub port: u16,
    /// The libpq-shaped URL `sqlx` connects with, derived from `DB_URL`.
    ///
    /// Carries the username and password inline (`to_libpq_url` put them
    /// there), which is exactly why [`Config`] does not derive `Debug`: this
    /// process talks to Supabase Postgres in `prod`, and nothing stops a
    /// future `tracing::info!(?config)` from putting that password in the
    /// logs. See the manual `impl Debug` below.
    pub database_url: String,
    /// HikariCP's default, stated explicitly because it is the number
    /// `MatchResultCache`'s entire rationale is written against: one match
    /// write can tell 200 tabs to refetch, and the cache is what stops that
    /// becoming hundreds of concurrent six-query reads against *ten*
    /// connections.
    pub max_connections: u32,
    pub scraper_base_url: String,
    pub supabase_jwks_uri: Option<String>,
    /// Only needed when a frontend calls the API cross-origin instead of
    /// through the Cloudflare Worker's same-origin `/api/*` proxy.
    pub frontend_origin: Option<String>,
    pub rate_limit: RateLimitConfig,
}

/// Redacts `database_url`'s userinfo and prints every other field unchanged.
///
/// A bare `"<redacted>"` for the whole field would also work, but the host,
/// port, database name and query string are not secret and are exactly what
/// you'd reach for `?config` to check -- which host did this process actually
/// connect to. Only the credential is the thing that must never reach a log.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("profiles", &self.profiles)
            .field("port", &self.port)
            .field("database_url", &redact_userinfo(&self.database_url))
            .field("max_connections", &self.max_connections)
            .field("scraper_base_url", &self.scraper_base_url)
            .field("supabase_jwks_uri", &self.supabase_jwks_uri)
            .field("frontend_origin", &self.frontend_origin)
            .field("rate_limit", &self.rate_limit)
            .finish()
    }
}

/// `scheme://user:pass@host:port/db?query` -> `scheme://<redacted>@host:port/db?query`.
/// A URL with no userinfo (or no recognisable `scheme://` at all) passes
/// through untouched, since there is nothing in it to redact.
fn redact_userinfo(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let authority_start = scheme_end + "://".len();
    let authority_end = url[authority_start..]
        .find(['/', '?'])
        .map_or(url.len(), |i| authority_start + i);
    let Some(at) = url[authority_start..authority_end].rfind('@') else {
        return url.to_owned();
    };
    format!(
        "{}<redacted>@{}",
        &url[..authority_start],
        &url[authority_start + at + 1..]
    )
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub capacity: u64,
    pub refill_period: Duration,
    pub max_tracked_ips: u64,
    /// CIDR ranges whose `X-Forwarded-For` is believed. The defaults cover a
    /// reverse proxy on the same host -- loopback is not enough, because
    /// publishing the container as `127.0.0.1:8080:8080` still NATs the
    /// connection through the Docker bridge gateway.
    pub trusted_proxies: Vec<String>,
}

impl Config {
    pub fn is_prod(&self) -> bool {
        self.profiles.iter().any(|p| p == "prod")
    }

    /// True for `dev` and `test`, the two profiles whose Flyway locations
    /// include `db/seed` and whose auth is the `X-Manager-Id` stub.
    pub fn is_dev_like(&self) -> bool {
        !self.is_prod()
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        let raw: RawConfig = Figment::new()
            .merge(Serialized::defaults(RawConfig::default()))
            .merge(Env::raw())
            .extract()
            .map_err(|e| ConfigError::Extract(e.to_string()))?;
        raw.into_config()
    }
}

/// The flat env-shaped view. Field names are the environment variable names
/// lowercased, which is exactly how `Env::raw()` presents them.
///
/// Same exposure `Config` has, one field earlier: `db_password` is the plain
/// credential `to_libpq_url` has not yet folded into a URL. Not derived, for
/// the same reason -- see the manual `impl Debug` below.
#[derive(Clone, Serialize, Deserialize)]
struct RawConfig {
    spring_profiles_active: String,
    server_port: u16,
    db_url: String,
    db_user: String,
    db_password: String,
    db_max_connections: u32,
    scraper_base_url: String,
    supabase_jwks_uri: String,
    frontend_origin: String,
    rate_limit_api_capacity: u64,
    /// ISO-8601, as Spring's `Duration` binding accepts it (`PT1M`).
    rate_limit_api_refill_period: String,
    rate_limit_api_max_tracked_ips: u64,
    /// Comma-separated CIDRs.
    rate_limit_api_trusted_proxies: String,
}

impl std::fmt::Debug for RawConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawConfig")
            .field("spring_profiles_active", &self.spring_profiles_active)
            .field("server_port", &self.server_port)
            .field("db_url", &self.db_url)
            .field("db_user", &self.db_user)
            .field("db_password", &"<redacted>")
            .field("db_max_connections", &self.db_max_connections)
            .field("scraper_base_url", &self.scraper_base_url)
            .field("supabase_jwks_uri", &self.supabase_jwks_uri)
            .field("frontend_origin", &self.frontend_origin)
            .field("rate_limit_api_capacity", &self.rate_limit_api_capacity)
            .field(
                "rate_limit_api_refill_period",
                &self.rate_limit_api_refill_period,
            )
            .field(
                "rate_limit_api_max_tracked_ips",
                &self.rate_limit_api_max_tracked_ips,
            )
            .field(
                "rate_limit_api_trusted_proxies",
                &self.rate_limit_api_trusted_proxies,
            )
            .finish()
    }
}

impl Default for RawConfig {
    fn default() -> Self {
        Self {
            spring_profiles_active: String::new(),
            server_port: 8080,
            db_url: "jdbc:postgresql://localhost:5433/umfl".to_owned(),
            db_user: "umfl".to_owned(),
            db_password: "umfl".to_owned(),
            db_max_connections: 10,
            scraper_base_url: "http://localhost:3000".to_owned(),
            supabase_jwks_uri: String::new(),
            frontend_origin: String::new(),
            rate_limit_api_capacity: 300,
            rate_limit_api_refill_period: "PT1M".to_owned(),
            rate_limit_api_max_tracked_ips: 100_000,
            rate_limit_api_trusted_proxies:
                "127.0.0.1/32,::1/128,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16".to_owned(),
        }
    }
}

impl RawConfig {
    fn into_config(self) -> Result<Config, ConfigError> {
        // A capacity of zero is not a stricter throttle, it is a throttle
        // that admits nothing -- indistinguishable from the API being down --
        // so it is refused here as a misconfiguration rather than accepted as
        // a mode anyone wants. It also used to reach `RateLimiter::try_consume`
        // and divide-by-zero its way into a panic on the very first throttled
        // request; see the `try_from_secs_f64` fallback in `ratelimit.rs` for
        // the belt-and-braces half of that fix, which holds even for a
        // `RateLimitConfig` built directly (as the unit tests there do),
        // bypassing this check.
        if self.rate_limit_api_capacity == 0 {
            return Err(ConfigError::RateLimitCapacity);
        }
        Ok(Config {
            profiles: split_list(&self.spring_profiles_active),
            port: self.server_port,
            database_url: to_libpq_url(&self.db_url, &self.db_user, &self.db_password)?,
            max_connections: self.db_max_connections,
            scraper_base_url: self.scraper_base_url,
            supabase_jwks_uri: non_empty(self.supabase_jwks_uri),
            frontend_origin: non_empty(self.frontend_origin),
            rate_limit: RateLimitConfig {
                capacity: self.rate_limit_api_capacity,
                refill_period: parse_iso8601_duration(&self.rate_limit_api_refill_period)?,
                max_tracked_ips: self.rate_limit_api_max_tracked_ips,
                trusted_proxies: split_list(&self.rate_limit_api_trusted_proxies),
            },
        })
    }
}

fn non_empty(s: String) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn split_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `jdbc:postgresql://host:5432/db?sslmode=require` + user + password
/// -> `postgres://user:pass@host:5432/db?sslmode=require`.
///
/// A URL that is already libpq-shaped passes through untouched, so a future
/// deployment may set either form.
pub fn to_libpq_url(db_url: &str, user: &str, password: &str) -> Result<String, ConfigError> {
    if db_url.starts_with("postgres://") || db_url.starts_with("postgresql://") {
        return Ok(db_url.to_owned());
    }
    let Some(rest) = db_url.strip_prefix("jdbc:postgresql://") else {
        return Err(ConfigError::DbUrl(format!(
            "DB_URL must start with `jdbc:postgresql://` or `postgres://`, got `{db_url}`"
        )));
    };
    let user = utf8_percent_encode(user, USERINFO);
    let password = utf8_percent_encode(password, USERINFO);
    Ok(format!("postgres://{user}:{password}@{rest}"))
}

/// The subset of ISO-8601 durations Spring's `Duration` binding is actually
/// given here: `PT1M`, `PT30S`, `PT1H30M`. Days and anything calendar-shaped
/// are rejected rather than guessed at.
fn parse_iso8601_duration(text: &str) -> Result<Duration, ConfigError> {
    let bad = || ConfigError::Duration(text.to_owned());
    let body = text
        .strip_prefix("PT")
        .or_else(|| text.strip_prefix("pt"))
        .ok_or_else(bad)?;
    if body.is_empty() {
        return Err(bad());
    }

    let mut total = 0f64;
    let mut number = String::new();
    for ch in body.chars() {
        match ch {
            '0'..='9' | '.' => number.push(ch),
            'H' | 'h' | 'M' | 'm' | 'S' | 's' => {
                let value: f64 = number.parse().map_err(|_| bad())?;
                number.clear();
                total += value
                    * match ch {
                        'H' | 'h' => 3600.0,
                        'M' | 'm' => 60.0,
                        _ => 1.0,
                    };
            }
            _ => return Err(bad()),
        }
    }
    if !number.is_empty() {
        return Err(bad());
    }
    // `Duration::from_secs_f64` panics on either failure mode here: a finite
    // `total` too large to represent (an operator-set env var is one typo
    // away -- see the test below) and a non-finite one (a number literal past
    // `f64::MAX` parses as `inf` before it ever reaches this line). This is
    // the same "reject rather than guess" category as `P1D` and a bare `60`
    // above -- `try_from_secs_f64` is the non-panicking twin of the call this
    // function already returns `Result` for.
    Duration::try_from_secs_f64(total).map_err(|_| bad())
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read configuration: {0}")]
    Extract(String),
    #[error("{0}")]
    DbUrl(String),
    #[error("not an ISO-8601 duration: `{0}`")]
    Duration(String),
    #[error(
        "RATE_LIMIT_API_CAPACITY must be greater than 0 -- a zero-capacity throttle admits no \
         requests at all, which is a misconfiguration rather than a mode anyone wants"
    )]
    RateLimitCapacity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_userinfo_strips_the_credential_but_keeps_the_rest_of_the_url() {
        assert_eq!(
            redact_userinfo(
                "postgres://umfl:s3cret@db.example.supabase.co:5432/postgres?sslmode=require"
            ),
            "postgres://<redacted>@db.example.supabase.co:5432/postgres?sslmode=require"
        );
    }

    #[test]
    fn redact_userinfo_passes_a_url_with_no_credential_through_untouched() {
        assert_eq!(
            redact_userinfo("postgres://db.example.supabase.co:5432/postgres"),
            "postgres://db.example.supabase.co:5432/postgres"
        );
    }

    /// The whole point: a password in `Config` must never reach whatever
    /// formats `{:?}` on it (a future `tracing::info!(?config)`, most
    /// plausibly), while the host stays visible because that's what makes
    /// `?config` worth reaching for in the first place.
    #[test]
    fn configs_debug_output_never_contains_the_database_password() {
        let config = Config {
            profiles: vec!["prod".to_owned()],
            port: 8080,
            database_url: "postgres://umfl:s3cret@db.example.supabase.co:5432/postgres".to_owned(),
            max_connections: 10,
            scraper_base_url: "http://localhost:3000".to_owned(),
            supabase_jwks_uri: None,
            frontend_origin: None,
            rate_limit: RateLimitConfig {
                capacity: 300,
                refill_period: Duration::from_secs(60),
                max_tracked_ips: 100_000,
                trusted_proxies: vec![],
            },
        };

        let debug = format!("{config:?}");
        assert!(
            !debug.contains("s3cret"),
            "the password leaked into Debug output: {debug}"
        );
        assert!(
            debug.contains("db.example.supabase.co"),
            "the host should stay visible for debuggability: {debug}"
        );
    }

    /// Same exposure, one layer down: `RawConfig::db_password` is the plain
    /// credential before `to_libpq_url` folds it into a URL at all.
    #[test]
    fn raw_configs_debug_output_never_contains_the_database_password() {
        let raw = RawConfig {
            db_password: "s3cret".to_owned(),
            ..RawConfig::default()
        };

        let debug = format!("{raw:?}");
        assert!(
            !debug.contains("s3cret"),
            "the password leaked into Debug output: {debug}"
        );
        assert!(debug.contains("<redacted>"), "{debug}");
    }

    #[test]
    fn converts_the_jdbc_url_the_vps_env_already_carries() {
        assert_eq!(
            to_libpq_url(
                "jdbc:postgresql://db.example.supabase.co:5432/postgres?sslmode=require",
                "umfl",
                "s3cret"
            )
            .unwrap(),
            "postgres://umfl:s3cret@db.example.supabase.co:5432/postgres?sslmode=require"
        );
    }

    #[test]
    fn matches_the_application_yml_default() {
        assert_eq!(
            to_libpq_url("jdbc:postgresql://localhost:5433/umfl", "umfl", "umfl").unwrap(),
            "postgres://umfl:umfl@localhost:5433/umfl"
        );
    }

    /// A password with `@` or `/` in it would otherwise reshape the URL and
    /// point the process at a different host entirely.
    #[test]
    fn percent_encodes_the_userinfo() {
        let url = to_libpq_url("jdbc:postgresql://db:5432/umfl", "a b", "p@ss/w:rd").unwrap();
        assert_eq!(url, "postgres://a%20b:p%40ss%2Fw%3Ard@db:5432/umfl");
    }

    #[test]
    fn passes_a_libpq_url_through_untouched() {
        let url = "postgres://u:p@h/db";
        assert_eq!(to_libpq_url(url, "ignored", "ignored").unwrap(), url);
    }

    #[test]
    fn rejects_a_url_of_neither_shape() {
        assert!(to_libpq_url("mysql://h/db", "u", "p").is_err());
    }

    #[test]
    fn parses_the_durations_application_yml_uses() {
        assert_eq!(
            parse_iso8601_duration("PT1M").unwrap(),
            Duration::from_secs(60)
        );
        assert_eq!(
            parse_iso8601_duration("PT30S").unwrap(),
            Duration::from_secs(30)
        );
        assert_eq!(
            parse_iso8601_duration("PT1H30M").unwrap(),
            Duration::from_secs(5400)
        );
        assert!(parse_iso8601_duration("P1D").is_err());
        assert!(parse_iso8601_duration("60").is_err());
    }

    /// Reproduces, then closes, the panic this function used to have before it
    /// switched from `Duration::from_secs_f64` to `Duration::try_from_secs_f64`.
    /// Confirmed directly against `core::time` first, so this test is honest
    /// about what it is proving rather than about this function's plumbing:
    /// `Duration::from_secs_f64` panics outright both on a finite value too
    /// large to represent and on a non-finite one ("value is either too big or
    /// NaN"). `RATE_LIMIT_API_REFILL_PERIOD` is an operator-set env var, so an
    /// absurd hour count -- the PORTING.md-cited `...999H` -- is one typo away.
    #[test]
    fn an_overflowing_or_non_finite_duration_is_rejected_not_panicked() {
        // The hazard, confirmed at the primitive `core::time` gave us, so a
        // future change to that stdlib behaviour would fail this assertion
        // rather than leave the rest of the test silently proving nothing.
        assert!(
            std::panic::catch_unwind(|| Duration::from_secs_f64(1e30)).is_err(),
            "Duration::from_secs_f64 itself must still panic on overflow, or this test is stale"
        );
        assert!(
            std::panic::catch_unwind(|| Duration::from_secs_f64(f64::INFINITY)).is_err(),
            "Duration::from_secs_f64 itself must still panic on a non-finite value, or this test is stale"
        );

        // Finite (huge, but well under f64::MAX) and still overflows Duration
        // once multiplied out -- the PORTING.md example.
        assert!(parse_iso8601_duration("PT99999999999999999999999999999H").is_err());
        // A number literal past f64::MAX itself parses as `inf`, driving
        // `total` non-finite before it ever reaches the `Duration` conversion.
        assert!(parse_iso8601_duration(&format!("PT{}H", "9".repeat(400))).is_err());
    }

    /// A zero capacity used to reach `RateLimiter::try_consume` and divide by
    /// its own zero refill rate -- see `ratelimit.rs`. Rejecting it here,
    /// before a `RateLimitConfig` is ever built, is the primary fix; that
    /// module's `try_from_secs_f64` fallback is the belt-and-braces half for
    /// a caller who bypasses this validation, as its own unit tests do.
    #[test]
    fn a_zero_rate_limit_capacity_is_rejected_at_config_time() {
        let raw = RawConfig {
            rate_limit_api_capacity: 0,
            ..RawConfig::default()
        };
        assert!(matches!(
            raw.into_config(),
            Err(ConfigError::RateLimitCapacity)
        ));
    }

    #[test]
    fn defaults_reproduce_rate_limit_properties() {
        let config = RawConfig::default().into_config().unwrap();
        assert_eq!(config.rate_limit.capacity, 300);
        assert_eq!(config.rate_limit.refill_period, Duration::from_secs(60));
        assert_eq!(config.rate_limit.max_tracked_ips, 100_000);
        assert_eq!(
            config.rate_limit.trusted_proxies,
            [
                "127.0.0.1/32",
                "::1/128",
                "10.0.0.0/8",
                "172.16.0.0/12",
                "192.168.0.0/16"
            ]
        );
        assert_eq!(config.max_connections, 10);
        assert!(config.profiles.is_empty());
        assert!(config.is_dev_like());
    }

    #[test]
    fn splits_the_profile_list_the_way_spring_does() {
        assert_eq!(split_list("dev"), ["dev"]);
        assert_eq!(split_list("dev, test"), ["dev", "test"]);
        assert!(split_list("").is_empty());
    }
}
