//! The Supabase bearer token: the `prod` credential.
//!
//! Oracle: `auth/SupabaseAuthenticationConverter.kt`, plus the explicit
//! `NimbusJwtDecoder` bean in `config/SecurityConfig.kt`.
//!
//! Supabase Auth mints this token whatever the upstream identity provider --
//! Discord OAuth is only how the end user authenticated *with Supabase*, and
//! this backend never talks to Discord. The `sub` claim is the stable
//! `auth.users.id` UUID, and it is the only claim that decides identity.
//!
//! **ES256, not RS256.** Boot's autoconfigured decoder, built from the
//! `jwk-set-uri` property alone, accepts RS256 only; Supabase's current "JWT
//! Signing Keys" projects sign with ES256, so that decoder rejects every real
//! token with "no matching key(s) found" even though the `kid` is right there
//! in the JWKS. The Kotlin works around it with a hand-built decoder naming the
//! algorithm; here the algorithm is simply named.

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
        Self {
            inner: Arc::new(RwLock::new(None)),
            last_fetch: Arc::new(RwLock::new(None)),
            client: reqwest::Client::new(),
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

    // `JwtValidators.createDefault()`: expiry and not-before, with Nimbus'
    // 60-second clock skew, and **no** audience or issuer check -- none is
    // configured on the Kotlin decoder either.
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
/// identity has no linked `manager` row yet.
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

/// The Discord profile claims Supabase surfaces in `user_metadata`, with the
/// same fallback chain the Kotlin uses.
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
/// Kotlin's `Char.isLetterOrDigit` is Unicode-aware, so `char::is_alphanumeric`
/// is the faithful filter rather than `is_ascii_alphanumeric`.
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
