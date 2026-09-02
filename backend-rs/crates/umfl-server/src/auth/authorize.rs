//! Which routes need an identity. **The only place that decides it.**
//!
//! Oracle: the private `apiAuthorizationRules` in `config/SecurityConfig.kt`,
//! passed to `authorizeHttpRequests` by *both* chains.
//!
//! `AGENTS.md` is emphatic that the two chains differ only in how a credential
//! is verified and never in what it unlocks, and the Kotlin buys that by
//! sharing one function between them. Here there is one function and no chains
//! at all, so the property is structural: [`RULES`] is the table, and both
//! profiles walk it.
//!
//! # Why one middleware over the raw path, and not per-route layers
//!
//! `anyRequest().denyAll()` has to answer for paths matching **no** route. A
//! per-route layer never runs for those, so axum would 404 where Spring
//! answers 401 or 403. Verified against the running Kotlin backend:
//!
//! ```text
//! GET /nope                       -> 401   (anonymous)
//! GET /nope     X-Manager-Id: 2   -> 403   (authenticated, but nothing permits it)
//! GET /api/nope                   -> 401   (anonymous; `/api/**` needs an identity)
//! GET /api/nope X-Manager-Id: 2   -> 404   (authenticated, so it reaches the router)
//! ```
//!
//! That last line is the one a per-route layer could not reproduce and is worth
//! keeping in mind: a rule that *permits* hands the request on to the router,
//! which may still 404. Authorization is not routing.
//!
//! # Ant patterns
//!
//! Spring's matchers are Ant paths, where `*` matches within one segment and
//! `**` matches any number of them. `/api/tournaments/*` therefore matches
//! `/api/tournaments/1` and **not** `/api/tournaments/1/standings` -- which is
//! exactly why the deeper public GETs are listed individually below.

use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;

use crate::error::ApiError;
use crate::manager::Manager;

/// What a matched rule demands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// `permitAll()`
    Permit,
    /// `authenticated()`
    Authenticated,
    /// `hasRole("ADMIN")` -- from `managers.is_admin`, never from a token claim.
    Admin,
    /// `denyAll()`
    Deny,
}

/// One row of `apiAuthorizationRules`, in order. **First match wins**, so the
/// order here is as load-bearing as it is there -- `/api/admin/**` must precede
/// `/api/**` or an admin route would only ever require an identity.
pub struct Rule {
    /// `None` matches any method, as Spring's method-less `requestMatchers`
    /// does.
    pub method: Option<Method>,
    pub pattern: &'static str,
    pub access: Access,
}

/// The table, transcribed rule for rule from `apiAuthorizationRules`.
///
/// Keep it in step with `authorize_rules` in `tests/it/security.rs`, which
/// asserts it from the outside, exactly as `SecurityConfigTest` and
/// `DevSecurityConfigTest` do.
pub fn rules() -> &'static [Rule] {
    static RULES: std::sync::OnceLock<Vec<Rule>> = std::sync::OnceLock::new();
    RULES.get_or_init(|| {
        vec![
            Rule {
                method: None,
                pattern: "/actuator/health",
                access: Access::Permit,
            },
            Rule {
                method: None,
                pattern: "/actuator/info",
                access: Access::Permit,
            },
            // Viewing tournaments, hero pools and standings needs no account --
            // only entering a tournament and drafting a roster does. GET only:
            // `POST /api/tournaments` falls through to `/api/**` below.
            Rule {
                method: Some(Method::GET),
                pattern: "/api/tournaments/*/heroes",
                access: Access::Permit,
            },
            Rule {
                method: Some(Method::GET),
                pattern: "/api/tournaments",
                access: Access::Permit,
            },
            Rule {
                method: Some(Method::GET),
                pattern: "/api/tournaments/*",
                access: Access::Permit,
            },
            Rule {
                method: Some(Method::GET),
                pattern: "/api/tournaments/*/standings",
                access: Access::Permit,
            },
            Rule {
                method: Some(Method::GET),
                pattern: "/api/tournaments/*/standings/stream",
                access: Access::Permit,
            },
            Rule {
                method: Some(Method::GET),
                pattern: "/api/tournaments/*/matches",
                access: Access::Permit,
            },
            // Must precede `/api/**` -- first match wins.
            Rule {
                method: None,
                pattern: "/api/admin/**",
                access: Access::Admin,
            },
            Rule {
                method: None,
                pattern: "/api/**",
                access: Access::Authenticated,
            },
            // `anyRequest().denyAll()`.
            Rule {
                method: None,
                pattern: "/**",
                access: Access::Deny,
            },
        ]
    })
}

/// The access the table demands for this request.
pub fn required_access(method: &Method, path: &str) -> Access {
    rules()
        .iter()
        .find(|rule| {
            rule.method.as_ref().is_none_or(|m| m == method) && matches_ant(rule.pattern, path)
        })
        .map_or(Access::Deny, |rule| rule.access)
}

/// Spring's `AntPathMatcher`, restricted to what the table actually uses: a
/// literal, a single-segment `*`, and a trailing `**`.
///
/// Deliberately not a general Ant implementation. Every pattern above is one of
/// those three shapes, and a general matcher would be a pile of untested
/// behaviour standing behind the application's only authorization decision.
fn matches_ant(pattern: &str, path: &str) -> bool {
    let mut pattern_segments = pattern.split('/');
    let mut path_segments = path.split('/');

    loop {
        match (pattern_segments.next(), path_segments.next()) {
            (Some("**"), _) => return true,
            (Some(p), Some(s)) => {
                if p != "*" && p != s {
                    return false;
                }
            }
            (None, None) => return true,
            // A `*` never matches an absent segment, and a path longer than its
            // pattern only matches through a `**`, which returned above.
            (Some(_), None) | (None, Some(_)) => return false,
        }
    }
}

/// Enforces the table.
///
/// Runs after [`authenticate`][super::authenticate], so the [`Manager`] is
/// already in the extensions if the request offered a usable credential.
///
/// **A denial's status depends on who is asking**, which is
/// `ExceptionTranslationFilter`'s behaviour and not an embellishment: an
/// anonymous request gets the authentication entry point's 401 ("you have not
/// said who you are"), an authenticated one gets the access-denied handler's
/// 403 ("you have, and it is not enough"). Both bodies are rendered without an
/// `instance` field, because in Kotlin neither ever reaches Spring MVC.
pub async fn authorize(req: Request, next: Next) -> Response {
    let manager = req.extensions().get::<Manager>();
    let access = required_access(req.method(), req.uri().path());

    let allowed = match access {
        Access::Permit => true,
        Access::Authenticated => manager.is_some(),
        Access::Admin => manager.is_some_and(|m| m.is_admin),
        Access::Deny => false,
    };
    if allowed {
        return next.run(req).await;
    }

    super::reject(if manager.is_some() {
        ApiError::Forbidden
    } else {
        ApiError::Unauthorized
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_star_stays_inside_one_segment() {
        assert!(matches_ant("/api/tournaments/*", "/api/tournaments/1"));
        assert!(!matches_ant(
            "/api/tournaments/*",
            "/api/tournaments/1/standings"
        ));
        assert!(!matches_ant("/api/tournaments/*", "/api/tournaments"));
    }

    #[test]
    fn a_trailing_double_star_matches_any_depth_including_none() {
        assert!(matches_ant("/api/admin/**", "/api/admin/heroes"));
        assert!(matches_ant(
            "/api/admin/**",
            "/api/admin/tournaments/1/matches"
        ));
        assert!(matches_ant("/api/admin/**", "/api/admin/"));
        assert!(!matches_ant("/api/admin/**", "/api/heroes"));
    }

    #[test]
    fn a_literal_matches_only_itself() {
        assert!(matches_ant("/actuator/health", "/actuator/health"));
        assert!(!matches_ant("/actuator/health", "/actuator/healthz"));
        assert!(!matches_ant("/actuator/health", "/actuator"));
    }

    /// The public reads `AGENTS.md` names: browsing a tournament needs no
    /// account, entering one does.
    #[test]
    fn viewing_a_tournament_needs_no_account() {
        for path in [
            "/api/tournaments",
            "/api/tournaments/1",
            "/api/tournaments/1/heroes",
            "/api/tournaments/1/standings",
            "/api/tournaments/1/standings/stream",
            "/api/tournaments/1/matches",
        ] {
            assert_eq!(
                required_access(&Method::GET, path),
                Access::Permit,
                "GET {path}"
            );
        }
    }

    /// The `permitAll` rules are GET-only, so a write to the same path falls
    /// through to `/api/**`. Confirmed against the Kotlin: `POST
    /// /api/tournaments` and `PUT /api/tournaments/1` both 401 anonymously.
    #[test]
    fn a_write_to_a_publicly_readable_path_still_needs_an_identity() {
        assert_eq!(
            required_access(&Method::POST, "/api/tournaments"),
            Access::Authenticated
        );
        assert_eq!(
            required_access(&Method::PUT, "/api/tournaments/1"),
            Access::Authenticated
        );
        assert_eq!(
            required_access(&Method::POST, "/api/tournaments/1/entries"),
            Access::Authenticated
        );
    }

    #[test]
    fn admin_routes_outrank_the_general_api_rule() {
        assert_eq!(
            required_access(&Method::POST, "/api/admin/heroes"),
            Access::Admin
        );
        assert_eq!(
            required_access(&Method::GET, "/api/admin/tournaments/1/matches"),
            Access::Admin
        );
    }

    /// `anyRequest().denyAll()` -- and the reason authorization cannot be a
    /// per-route layer.
    #[test]
    fn a_path_matching_no_rule_is_denied_outright() {
        assert_eq!(required_access(&Method::GET, "/nope"), Access::Deny);
        assert_eq!(
            required_access(&Method::GET, "/actuator/metrics"),
            Access::Deny
        );
        assert_eq!(required_access(&Method::GET, "/"), Access::Deny);
    }

    /// An unrouted path *under* `/api` is a different case: it matches
    /// `/api/**`, so an authenticated caller is let through to the router and
    /// gets its 404.
    #[test]
    fn an_unrouted_api_path_merely_needs_an_identity() {
        assert_eq!(
            required_access(&Method::GET, "/api/nope"),
            Access::Authenticated
        );
    }

    #[test]
    fn health_and_info_are_public_but_the_rest_of_actuator_is_not() {
        assert_eq!(
            required_access(&Method::GET, "/actuator/health"),
            Access::Permit
        );
        assert_eq!(
            required_access(&Method::GET, "/actuator/info"),
            Access::Permit
        );
        assert_eq!(required_access(&Method::GET, "/actuator/env"), Access::Deny);
    }
}
