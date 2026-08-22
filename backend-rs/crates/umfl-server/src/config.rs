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

#[derive(Debug, Clone)]
pub struct Config {
    /// `SPRING_PROFILES_ACTIVE`, split on commas. Empty means the default
    /// profile: no seed data, prod-shaped security.
    pub profiles: Vec<String>,
    pub port: u16,
    /// The libpq-shaped URL `sqlx` connects with, derived from `DB_URL`.
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Ok(Duration::from_secs_f64(total))
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read configuration: {0}")]
    Extract(String),
    #[error("{0}")]
    DbUrl(String),
    #[error("not an ISO-8601 duration: `{0}`")]
    Duration(String),
}

#[cfg(test)]
mod tests {
    use super::*;

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
