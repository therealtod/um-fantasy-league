//! The Supabase bearer token: the `prod` credential.
//!
//! Supabase Auth mints this token whatever the upstream identity provider --
//! Discord OAuth is only how the end user authenticated *with Supabase*, and
//! this backend never talks to Discord. The `sub` claim is the stable
//! `auth.users.id` UUID, and it is the only claim that decides identity.
//!
//! **ES256, not RS256.** Supabase's current "JWT Signing Keys" projects sign
//! with ES256, so the decoder below names that algorithm explicitly rather
//! than leaving it to be inferred from the JWKS alone.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, header};
use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::ApiError;
use crate::manager::{self, Manager};
use crate::state::AppState;

/// `SupabaseAuthenticationConverter.MAX_PROVISION_ATTEMPTS`.
const MAX_PROVISION_ATTEMPTS: usize = 3;

/// How often an unknown `kid` may trigger a refetch.
///
/// `RemoteJWKSet` refreshes on a cache miss but rate-limits it, so a stream of
/// tokens naming a `kid` that genuinely does not exist cannot turn into a
/// stream of outbound requests to Supabase.
const MIN_REFETCH_INTERVAL: Duration = Duration::from_secs(30);

/// Short: either Supabase answers or it doesn't, and "doesn't" should surface
/// as a rejected token quickly rather than a request that never returns.
///
/// Contrast the scraper's 90s `SCRAPE_TIMEOUT` (`matchimport/scraper.rs`): that
/// call drives a real browser rendering a page and can legitimately take
/// close to a minute. This one is a small JSON GET against an identity
/// provider, sitting on the request path of *every* bearer-authenticated
/// call once the cache misses -- a different kind of call by an order of
/// magnitude, not just a smaller one.
const JWKS_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Same reasoning as [`JWKS_CONNECT_TIMEOUT`]: bound the whole round trip, not
/// just the connect, so a Supabase that accepts the connection and then
/// hangs (or is blackholed mid-response) can't stall the write-locked
/// [`JwksCache::refresh`] below indefinitely.
const JWKS_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// The signing keys, fetched on first use and refreshed when a token names a
/// `kid` this process has not seen.
///
/// Held in [`AppState`] rather than rebuilt per request: a fetch per token
/// would put a network round trip on every authenticated call, and Supabase
/// rotates these keys on the order of months.
#[derive(Clone)]
pub struct JwksCache {
    inner: Arc<RwLock<Option<JwkSet>>>,
    last_fetch: Arc<RwLock<Option<Instant>>>,
    client: reqwest::Client,
    uri: Option<String>,
}

impl JwksCache {
    pub fn new(uri: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(JWKS_CONNECT_TIMEOUT)
            .timeout(JWKS_REQUEST_TIMEOUT)
            .build()
            // Only fails if the TLS backend cannot be initialised, which is a
            // start-up fault rather than a request-time one -- same rationale
            // as `HttpScraperClient::new`.
            .expect("build the Supabase JWKS HTTP client");
        Self {
            inner: Arc::new(RwLock::new(None)),
            last_fetch: Arc::new(RwLock::new(None)),
            client,
            uri,
        }
    }

    /// The key for `kid`, refetching once if it is not already cached.
    async fn key(&self, kid: &str) -> Option<Jwk> {
        if let Some(jwk) = self.inner.read().await.as_ref().and_then(|s| s.find(kid)) {
            return Some(jwk.clone());
        }
        self.refresh().await;
        self.inner.read().await.as_ref()?.find(kid).cloned()
    }

    /// Fetches the key set and swaps it in, serialised by `last_fetch`'s write
    /// lock -- which is held across the network round trip below, not just
    /// the timestamp check, **on purpose**: every request whose `kid` misses
    /// the cache calls this, and holding the lock for the whole fetch is what
    /// collapses a burst of concurrent misses onto one outbound call instead
    /// of N. Read [`key`](Self::key) queues on this same lock during that
    /// window, so callers stall, not stack.
    ///
    /// That is only safe because `client` above carries `JWKS_CONNECT_TIMEOUT`
    /// / `JWKS_REQUEST_TIMEOUT`: an unbounded client would turn "one fetch for
    /// everyone" into "everyone hangs together" the moment Supabase's JWKS
    /// endpoint stalls or blackholes a connection. The lock and the timeout
    /// are one decision -- removing the timeout without also removing the
    /// held-lock structure reopens the stall this pair exists to prevent.
    async fn refresh(&self) {
        let Some(uri) = self.uri.as_deref() else {
            // `SUPABASE_JWKS_URI` unset. Every token is then unverifiable, which
            // is the same 401 Boot's decoder produced with no `jwk-set-uri`.
            return;
        };
        {
            let last = self.last_fetch.read().await;
            if last.is_some_and(|at| at.elapsed() < MIN_REFETCH_INTERVAL) {
                return;
            }
        }
        let mut last = self.last_fetch.write().await;
        // Re-checked under the write lock: several requests can queue on it.
        if last.is_some_and(|at| at.elapsed() < MIN_REFETCH_INTERVAL) {
            return;
        }
        *last = Some(Instant::now());

        match self.client.get(uri).send().await {
            Ok(response) => match response.json::<JwkSet>().await {
                Ok(set) => *self.inner.write().await = Some(set),
                Err(err) => tracing::error!(error = %err, uri, "JWKS response was not a key set"),
            },
            Err(err) => tracing::error!(error = %err, uri, "Could not fetch the JWKS"),
        }
    }
}

/// The claims this application reads. Everything else in a Supabase token --
/// `aud`, `role`, `app_metadata`, the session id -- is deliberately ignored:
/// `sub` decides identity and `is_admin` decides authority, and neither is
/// negotiable by the token's issuer.
#[derive(Debug, Deserialize)]
struct Claims {
    sub: Option<String>,
    #[serde(default)]
    user_metadata: Option<serde_json::Value>,
}

/// `None` for an anonymous request, `Err` for a credential that is present and
/// unusable.
pub async fn resolve(state: &AppState, headers: &HeaderMap) -> Result<Option<Manager>, ApiError> {
    let Some(token) = bearer_token(headers) else {
        return Ok(None);
    };

    let claims = verify(state, token).await?;
    let manager = resolve_manager(state, &claims).await?;
    Ok(Some(manager))
}

/// `BearerTokenAuthenticationFilter`'s half: a token is offered only when the
/// scheme is exactly `Bearer`. Any other scheme is not this chain's credential
/// and leaves the request anonymous.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme.eq_ignore_ascii_case("Bearer").then(|| token.trim())
}

async fn verify(state: &AppState, token: &str) -> Result<Claims, ApiError> {
    let header = jsonwebtoken::decode_header(token).map_err(|err| {
        tracing::warn!(error = %err, "Rejected bearer token: unreadable header");
        ApiError::Unauthorized
    })?;
    let Some(kid) = header.kid.as_deref() else {
        tracing::warn!("Rejected bearer token: no kid");
        return Err(ApiError::Unauthorized);
    };
    let Some(jwk) = state.jwks.key(kid).await else {
        tracing::warn!(kid, "Rejected bearer token: no matching key(s) found");
        return Err(ApiError::Unauthorized);
    };
    let key = DecodingKey::from_jwk(&jwk).map_err(|err| {
        tracing::warn!(error = %err, kid, "Rejected bearer token: unusable JWK");
        ApiError::Unauthorized
    })?;

    // Expiry and not-before, with a 60-second clock skew allowance, and
    // **no** audience or issuer check -- neither is configured below.
    let mut validation = Validation::new(Algorithm::ES256);
    validation.leeway = 60;
    validation.validate_aud = false;

    jsonwebtoken::decode::<Claims>(token, &key, &validation)
        .map(|data| data.claims)
        .map_err(|err| {
            // The one place the real reason is observable. The body stays
            // generic, matching Bearer's convention of handing the client no
            // token-validation specifics -- so without this line an expired
            // token, a JWKS mismatch and a bad signature are one silent 401.
            tracing::warn!(error = %err, "Rejected bearer token");
            ApiError::Unauthorized
        })
}

async fn resolve_manager(state: &AppState, claims: &Claims) -> Result<Manager, ApiError> {
    let Some(sub) = claims.sub.as_deref() else {
        tracing::warn!("Rejected bearer token: missing sub claim");
        return Err(ApiError::Unauthorized);
    };
    let Ok(auth_user_id) = Uuid::parse_str(sub) else {
        tracing::warn!("Rejected bearer token: malformed sub claim: {sub}");
        return Err(ApiError::Unauthorized);
    };

    if let Some(manager) = manager::query::find_by_auth_user_id(&state.pool, auth_user_id).await? {
        return Ok(manager);
    }
    provision(state, claims, auth_user_id).await
}

/// Just-in-time provisioning: the first request from a brand-new Supabase
/// identity has no linked `managers` row yet.
///
/// There is no starting balance to hand out -- budget is granted per tournament
/// registration -- so the row is a handle, a display name and the link.
///
/// The read-then-insert below has no lock, so two concurrent first requests
/// from the same new identity can both miss and both try to insert. One wins;
/// the other must recover rather than surface a 500. A unique violation means
/// either that race (re-reading by `auth_user_id` finds the row the winner just
/// committed) or two *different* new users whose derived handles collided
/// (re-reading still finds nothing, so retry with a freshly resolved handle).
async fn provision(
    state: &AppState,
    claims: &Claims,
    auth_user_id: Uuid,
) -> Result<Manager, ApiError> {
    let display_name = discord_username(claims, auth_user_id);

    for _ in 0..MAX_PROVISION_ATTEMPTS {
        let handle = unique_handle(state, &display_name).await?;
        match manager::writer::insert(&state.pool, &handle, &display_name, auth_user_id).await {
            Ok(manager) => return Ok(manager),
            Err(err) if is_unique_violation(&err) => {
                if let Some(winner) =
                    manager::query::find_by_auth_user_id(&state.pool, auth_user_id).await?
                {
                    return Ok(winner);
                }
            }
            Err(err) => return Err(ApiError::from_sqlx(err)),
        }
    }
    tracing::error!(%auth_user_id, "Unable to provision manager");
    Err(ApiError::Internal)
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.is_unique_violation())
}

/// The Discord profile claims Supabase surfaces in `user_metadata`, tried in
/// the fallback chain below.
fn discord_username(claims: &Claims, auth_user_id: Uuid) -> String {
    let metadata = claims.user_metadata.as_ref();
    let field = |name: &str| {
        metadata
            .and_then(|m| m.get(name))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    field("user_name")
        .or_else(|| field("full_name"))
        // `UUID.toString().take(8)` -- the first block of the UUID.
        .unwrap_or_else(|| format!("Manager-{}", &auth_user_id.to_string()[..8]))
}

/// `sanitized`, then `sanitized1`, `sanitized2`, ... until one is free.
///
/// `char::is_alphanumeric` is Unicode-aware, which matters for a Discord
/// display name that is not ASCII -- `is_ascii_alphanumeric` would strip more
/// than intended.
async fn unique_handle(state: &AppState, base: &str) -> Result<String, ApiError> {
    let sanitized: String = base.chars().filter(|c| c.is_alphanumeric()).collect();
    let sanitized = if sanitized.is_empty() {
        "Manager"
    } else {
        &sanitized
    };

    let mut candidate = sanitized.to_owned();
    let mut suffix = 1u32;
    while manager::query::find_by_handle(&state.pool, &candidate)
        .await?
        .is_some()
    {
        candidate = format!("{sanitized}{suffix}");
        suffix += 1;
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scenario the two changes above exist to prevent: a JWKS endpoint
    /// that accepts the TCP connection (so `JWKS_CONNECT_TIMEOUT` never
    /// fires) and then never writes a response. Before the client carried a
    /// request timeout, `refresh()` -- and the write lock it holds for the
    /// whole fetch, see its doc comment -- would have hung forever, and every
    /// request behind an unknown `kid` with it.
    ///
    /// `reqwest`'s configured timeouts aren't inspectable, so this is the
    /// honest version of that assertion: not "a timeout is set", but "the
    /// call this timeout guards actually returns". The outer
    /// `tokio::time::timeout` is only there so a regression fails this test
    /// instead of hanging the suite; it is deliberately looser than
    /// `JWKS_REQUEST_TIMEOUT` so the assertion is about the client's own
    /// bound, not a race against it.
    #[tokio::test]
    async fn refresh_returns_rather_than_hanging_against_an_endpoint_that_never_responds() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a local listener");
        let addr = listener.local_addr().expect("local addr");
        std::thread::spawn(move || {
            // Accept the connection and then just sit on it -- no response,
            // ever. The thread (and the socket) outlive the assertion below;
            // the test process exiting is what cleans it up.
            let _ = listener.accept();
            std::thread::sleep(Duration::from_secs(60));
        });

        let cache = JwksCache::new(Some(format!("http://{addr}/jwks")));

        tokio::time::timeout(
            JWKS_REQUEST_TIMEOUT + Duration::from_secs(5),
            cache.refresh(),
        )
        .await
        .expect(
            "refresh() did not return well within its own request timeout -- \
                 the HTTP client is unbounded again",
        );

        // The fetch failed (it timed out), so there is still nothing cached.
        assert!(cache.inner.read().await.is_none());
    }
}
