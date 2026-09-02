//! Who the request is, and whether that is enough.
//!
//! Oracle: `auth/*.kt`, `config/SecurityConfig.kt`,
//! `config/ProblemDetailAuthenticationEntryPoint.kt`,
//! `config/ProblemDetailAccessDeniedHandler.kt`.
//!
//! The arrangement `AGENTS.md` insists on survives the port intact, and it is
//! worth stating in the terms this crate uses:
//!
//! * **[`authenticate`] resolves an identity only when one is offered, and
//!   carries no route knowledge.** A request with no credential stays anonymous
//!   and costs no database query, exactly as an anonymous request in prod pays
//!   no JWT verification. Which routes actually needed one is
//!   [`authorize`][authorize::authorize]'s business alone.
//! * **[`authorize`][authorize::authorize] is the single place that decides
//!   which routes need an identity, for every profile.** The two credential
//!   paths ([`dev`] and [`supabase`]) differ in how a credential is *verified*
//!   and never in what it unlocks -- the property `SecurityConfigTest` and
//!   `DevSecurityConfigTest` assert from either side.
//! * **The admin role comes from `managers.is_admin`**, our own data, never from
//!   an identity-provider claim. That is `ManagerAuthorities`, which is one
//!   field access here rather than a class, because a `Manager` in the
//!   extensions already carries it.

pub mod authorize;
pub mod dev;
pub mod supabase;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;

use crate::error::ApiError;
use crate::manager::Manager;
use crate::state::AppState;

/// The calling [`Manager`], for a handler that requires one.
///
/// Replaces `@CurrentManager` on a non-null parameter. The 401 is unreachable
/// in practice -- [`authorize`][authorize::authorize] has already refused every
/// anonymous request to a route that injects this -- which is why the Kotlin
/// could get away with `error("No authenticated manager for this request")` and
/// a 500 there. A 401 is the same answer the layer above would have given, so
/// nothing observable changes and the unreachable case stops being a 500.
#[derive(Debug, Clone)]
pub struct CurrentManager(pub Manager);

/// The calling [`Manager`], or `None` for an anonymous request.
///
/// Replaces `@CurrentManager` on a *nullable* parameter, which the Kotlin
/// argument resolver routed to `currentOrNull()`.
#[derive(Debug, Clone)]
pub struct MaybeManager(pub Option<Manager>);

impl<S: Send + Sync> FromRequestParts<S> for CurrentManager {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Manager>()
            .cloned()
            .map(Self)
            .ok_or(ApiError::Unauthorized)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for MaybeManager {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(parts.extensions.get::<Manager>().cloned()))
    }
}

/// Resolves the request's identity, if it offered one, and puts the [`Manager`]
/// in the request extensions for [`authorize`][authorize::authorize] and the
/// extractors above.
///
/// Which of the two credential shapes is accepted is the profile's only
/// difference: a Supabase bearer token in `prod`
/// ([`SupabaseAuthenticationConverter`][supabase]), an `X-Manager-Id` header
/// everywhere else ([`DevManagerAuthenticationFilter`][dev]).
///
/// **Absent is not the same as bad.** No credential passes straight through as
/// anonymous; a credential that is present and unusable is rejected here with a
/// 401, because something *was* offered and it is wrong. That is the Kotlin's
/// `BadCredentialsException` reaching `ProblemDetailAuthenticationEntryPoint`.
pub async fn authenticate(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let resolved = if state.config.is_prod() {
        supabase::resolve(&state, req.headers()).await
    } else {
        dev::resolve(&state, req.headers()).await
    };

    match resolved {
        Ok(Some(manager)) => {
            req.extensions_mut().insert(manager);
        }
        Ok(None) => {}
        // Rendered here rather than returned as an `ApiError`, so the body
        // carries no `instance` -- see `ProblemDetail::into_response_without_instance`.
        Err(err) => return reject(err),
    }
    next.run(req).await
}

/// Renders a rejection raised *inside* the filter chain.
///
/// The Kotlin's entry point and access-denied handler write straight to the
/// servlet response and never reach Spring MVC, so their bodies have no
/// `instance` field. Going through `ApiError`'s own `IntoResponse` would add
/// one.
pub(crate) fn reject(err: ApiError) -> Response {
    err.problem().into_response_without_instance()
}
