//! The UMFL backend's I/O half: HTTP, SQL, auth, caching.
//!
//! Every domain rule lives in `umfl-domain`, which has no `sqlx`, no `axum`
//! and no `tokio` in its manifest. That is not a style preference -- it is
//! `AGENTS.md`'s "domain rules live in pure objects" turned into a build error.
//! A rule a component can state without a database belongs over there.

pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod hero;
pub mod http;
pub mod manager;
pub mod ratelimit;
pub mod state;
pub mod tournament;

use axum::Router;
use axum::http::{HeaderValue, Method, header};
use tower::ServiceBuilder;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowHeaders, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::state::AppState;

/// Assembles the whole application.
///
/// **Layer order is the Spring filter chain, and is load-bearing.** Outermost
/// first:
///
/// 1. `TraceLayer` -- so a request is logged whatever happens to it.
/// 2. `fill_problem_instance` -- outside `CatchPanicLayer`, so even the 500 a
///    panic produces carries the `instance` the servlet layer used to add.
/// 3. `rate_limit` -- **first of the security layers**, so a flood pays neither
///    a JWT verification nor a manager lookup. `addFilterBefore` put it ahead
///    of `BearerTokenAuthenticationFilter` for exactly this.
/// 4. `CorsLayer` -- ahead of authorization, so a preflight `OPTIONS` is
///    answered rather than denied. That is defect (c) fixed: `DevSecurityConfig`
///    had no `.cors()`, so a dev-profile preflight 401'd.
/// 5. `CatchPanicLayer` -- the `handleUnexpected` catch-all.
/// 6. `authenticate` -- resolves a credential **only if one is offered** and
///    never rejects for absence, carrying zero route knowledge.
/// 7. `authorize` -- walks the ordered rule table. One middleware over the raw
///    path rather than per-route layers, because `anyRequest().denyAll()` has to
///    answer for paths matching *no* route, where a per-route layer never runs
///    and axum would 404 where Spring answers 401/403.
pub fn build_router(state: AppState) -> Router {
    let cors = cors_layer(&state.config);

    api::routes()
        .method_not_allowed_fallback(http::method_not_allowed_fallback)
        .fallback(http::not_found_fallback)
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(axum::middleware::from_fn(http::fill_problem_instance))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    ratelimit::rate_limit,
                ))
                .layer(cors)
                .layer(CatchPanicLayer::custom(http::panic_response))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    auth::authenticate,
                ))
                .layer(axum::middleware::from_fn(auth::authorize::authorize)),
        )
        .with_state(state)
}

/// `CorsConfig.kt`: nothing at all unless `FRONTEND_ORIGIN` is set, and then
/// exactly one origin, five methods, any header, **no credentials**.
///
/// It stays unset in the deployed setup, where the Cloudflare Worker proxies
/// `/api/*` to the backend and the frontend's calls are same-origin.
fn cors_layer(config: &Config) -> CorsLayer {
    let Some(origin) = config.frontend_origin.as_deref() else {
        return CorsLayer::new();
    };
    let Ok(origin) = HeaderValue::from_str(origin) else {
        tracing::warn!(
            origin,
            "FRONTEND_ORIGIN is not a valid header value; CORS left disabled"
        );
        return CorsLayer::new();
    };
    CorsLayer::new()
        .allow_origin(origin)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(AllowHeaders::mirror_request())
        .expose_headers([header::CONTENT_TYPE])
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;
    use tower::ServiceExt;

    /// A router over a pool that has never connected.
    ///
    /// `connect_lazy` defers the first connection until a query runs, so every
    /// assertion below -- routing, the two fallbacks, the problem body, the
    /// `instance` field -- is exercised with no Postgres anywhere. The health
    /// endpoint is the only route that touches the pool, and it is not one of
    /// the routes tested here.
    fn router() -> Router {
        let config = Config::from_env().expect("defaults are valid");
        let pool = PgPoolOptions::new()
            .connect_lazy(&config.database_url)
            .expect("lazy pool");
        let rate_limiter = crate::ratelimit::RateLimiter::new(&config.rate_limit);
        let jwks = crate::auth::supabase::JwksCache::new(config.supabase_jwks_uri.clone());
        build_router(AppState {
            pool,
            config: Arc::new(config),
            rate_limiter,
            jwks,
        })
    }

    async fn get(uri: &str) -> (StatusCode, Option<String>, serde_json::Value) {
        request(Request::builder().uri(uri).body(Body::empty()).unwrap()).await
    }

    async fn request(req: Request<Body>) -> (StatusCode, Option<String>, serde_json::Value) {
        let response = router().oneshot(req).await.unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, content_type, body)
    }

    /// An unrouted path is **401, not 404** -- `anyRequest().denyAll()` denies
    /// it in the filter chain, long before Spring MVC could raise
    /// `NoResourceFoundException`.
    ///
    /// This was originally asserted as a 404 on the reasonable-looking grounds
    /// that `/nope` matches no route. Probing the running Kotlin backend says
    /// otherwise, and the difference is the whole reason authorization is one
    /// middleware over the raw path rather than a per-route layer:
    ///
    /// ```text
    /// GET /nope                      -> 401
    /// GET /nope    X-Manager-Id: 2   -> 403
    /// GET /api/nope                  -> 401
    /// GET /api/nope X-Manager-Id: 2  -> 404   ("/api/**" merely needs an identity)
    /// ```
    ///
    /// The two authenticated rows need a database and live in the integration
    /// suite; see `tests/it/security.rs`.
    #[tokio::test]
    async fn an_unrouted_path_is_denied_rather_than_answered() {
        let (status, content_type, body) = get("/nope").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(content_type.as_deref(), Some("application/problem+json"));
        assert_eq!(body["type"], "https://umfl.dev/problems/unauthorized");
        assert_eq!(body["title"], "Unauthorized");
        assert_eq!(body["status"], 401);
    }

    /// A rejection raised inside the filter chain carries **no** `instance`.
    ///
    /// `instance` is filled in by `RequestResponseBodyMethodProcessor`, which
    /// is Spring MVC and never sees a response the security filters wrote. So
    /// the 401 above has no `instance` while a handler's 404 does -- an added
    /// field would be as much a wire change as a dropped one.
    #[tokio::test]
    async fn a_filter_level_rejection_carries_no_instance() {
        let (_, _, body) = get("/nope").await;
        assert!(body.get("instance").is_none(), "{body}");
    }

    /// Spring filled `instance` from `HttpServletRequest.getRequestURI()` on
    /// its way out of `RequestResponseBodyMethodProcessor`. Losing it would be
    /// a wire change, and it is easy to lose: nothing else in axum supplies it.
    #[tokio::test]
    async fn every_problem_body_names_the_request_path() {
        // A publicly readable path, so the rule table lets it through to the
        // router -- where nothing is mounted yet, making this the 404 fallback
        // and therefore a *handler*-level document, which does carry `instance`.
        let (_, _, body) = get("/api/tournaments/1/standings").await;
        assert_eq!(body["instance"], "/api/tournaments/1/standings");
    }

    /// The 405 the base class used to produce. Before `GlobalExceptionHandler`
    /// inherited from `ResponseEntityExceptionHandler` this was a 500.
    #[tokio::test]
    async fn a_known_path_with_the_wrong_verb_is_a_405() {
        let req = Request::builder()
            .method("DELETE")
            .uri("/actuator/info")
            .body(Body::empty())
            .unwrap();
        let (status, _, body) = request(req).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(body["type"], "https://umfl.dev/problems/method-not-allowed");
        assert_eq!(body["detail"], "Method 'DELETE' is not supported.");
    }

    #[tokio::test]
    async fn actuator_info_is_empty_as_it_is_today() {
        let (status, _, body) = get("/actuator/info").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({}));
    }

    /// `default-property-inclusion: non_null` as a blanket assertion. The
    /// differential rig runs this same walk over every parity response body.
    #[tokio::test]
    async fn no_response_body_contains_a_json_null() {
        for uri in ["/nope", "/actuator/info"] {
            let (_, _, body) = get(uri).await;
            assert!(!body.to_string().contains("null"), "{uri} -> {body}");
        }
    }
}
