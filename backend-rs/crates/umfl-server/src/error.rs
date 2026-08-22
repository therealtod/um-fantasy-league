//! Every error this API returns, as an RFC 7807 problem detail.
//!
//! Oracle: `common/GlobalExceptionHandler.kt` and `common/DomainExceptions.kt`.
//!
//! The Kotlin's most awkward constraint does not survive the port and that is
//! fine: `ResponseEntityExceptionHandler`'s ambiguous-mapping trap exists
//! because `ExceptionHandlerExceptionResolver` runs ahead of
//! `DefaultHandlerExceptionResolver`, and axum has no such ordering. What does
//! survive is the *coverage* that inheritance bought — a malformed body, an
//! unparseable path variable, a wrong method and an unrouted path must each
//! answer with their real status and a `umfl` problem type rather than a 500 or
//! a bodiless 404. On this side that comes from [`ApiError::Framework`], the
//! extractors in [`crate::http::extract`], and the router's two fallbacks.
//!
//! **Every variant is defined here, in T0.** Feature tasks convert into these;
//! adding a variant later is a cross-cutting change that touches every owner's
//! merge, so it needs a reason, not a convenience.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use indexmap::IndexMap;
use umfl_domain::{DomainError, Violation};

use crate::http::problem::ProblemDetail;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The six domain exceptions, carrying their status mapping from
    /// `DomainExceptions.kt`'s KDoc.
    #[error("{0}")]
    Domain(#[from] DomainError),

    /// No credential at all on a route that needs one.
    ///
    /// Both the URL-matcher layer (`ProblemDetailAuthenticationEntryPoint`)
    /// and the method-security layer (`handleAuthentication`) render exactly
    /// this body, deliberately: which of the two denied a request is invisible
    /// to the client.
    #[error("authentication required")]
    Unauthorized,

    /// A credential that is not enough — the `hasRole('ADMIN')` denial.
    #[error("access denied")]
    Forbidden,

    /// Bean-validation failure on a request body, keyed by field path.
    ///
    /// The `fields` property is what `handleMethodArgumentNotValid` adds back
    /// on top of the plain 400 the base class would have produced.
    #[error("validation failed")]
    Validation(IndexMap<String, String>),

    /// A rejection Spring MVC raised from its own exception hierarchy, where
    /// the title and problem-type slug come from the status' reason phrase and
    /// the detail is Spring's own message, copied verbatim.
    ///
    /// See [`crate::http::extract`] for the messages and where each is raised.
    #[error("{detail}")]
    Framework { status: StatusCode, detail: String },

    /// Backstop for a constraint reaching the database unvalidated. Every
    /// known case is caught earlier by a domain policy, so this firing is a
    /// bug — hence the WARN in [`ApiError::log`].
    #[error("data integrity violation")]
    DataIntegrity,

    /// The genuinely unexpected. Logged at ERROR; the body says nothing.
    #[error("internal error")]
    Internal,
}

impl ApiError {
    /// Classifies a `sqlx` failure the way Spring's exception translation did:
    /// an integrity constraint becomes the 409 backstop, anything else is a
    /// 500.
    ///
    /// This is strictly better than the Kotlin, which substring-matched an
    /// exception message — `DatabaseError::constraint()` hands back the index
    /// name, so a feature service that wants to name a *specific* constraint
    /// (`uq_tournament_match_external_link`) should match on it and raise a
    /// [`DomainError::Conflict`] before this ever runs.
    pub fn from_sqlx(err: sqlx::Error) -> Self {
        match &err {
            sqlx::Error::Database(db)
                if db.is_unique_violation()
                    || db.is_foreign_key_violation()
                    || db.is_check_violation() =>
            {
                tracing::warn!(error = %err, constraint = db.constraint().unwrap_or("?"), "Data integrity violation reached the API layer unvalidated");
                Self::DataIntegrity
            }
            _ => {
                tracing::error!(error = %err, "Database error");
                Self::Internal
            }
        }
    }

    /// The problem document, minus `instance` — which the framework fills in
    /// from the request path once it knows it.
    pub fn problem(&self) -> ProblemDetail {
        match self {
            Self::Domain(DomainError::NotFound(m)) => ProblemDetail::new(
                StatusCode::NOT_FOUND,
                "Not found",
                Some(m.clone()),
                "not-found",
            ),
            Self::Domain(DomainError::Conflict(m)) => ProblemDetail::new(
                StatusCode::CONFLICT,
                "Conflict",
                Some(m.clone()),
                "conflict",
            ),
            Self::Domain(DomainError::ServiceUnavailable(m)) => ProblemDetail::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Service unavailable",
                Some(m.clone()),
                "service-unavailable",
            ),
            Self::Domain(e @ DomainError::RosterRule(v)) => {
                rule_problem("Roster rules violated", "roster-rule-violation", e, v)
            }
            Self::Domain(e @ DomainError::MatchRule(v)) => {
                rule_problem("Match result rules violated", "match-rule-violation", e, v)
            }
            Self::Domain(e @ DomainError::ScoringRule(v)) => rule_problem(
                "Scoring rule set rules violated",
                "scoring-rule-violation",
                e,
                v,
            ),
            Self::Unauthorized => ProblemDetail::new(
                StatusCode::UNAUTHORIZED,
                "Unauthorized",
                Some("Authentication is required to access this resource.".into()),
                "unauthorized",
            ),
            Self::Forbidden => ProblemDetail::new(
                StatusCode::FORBIDDEN,
                "Forbidden",
                Some("You do not have permission to perform this action.".into()),
                "forbidden",
            ),
            Self::Validation(fields) => ProblemDetail::new(
                StatusCode::BAD_REQUEST,
                "Invalid request",
                Some("One or more fields failed validation.".into()),
                "validation-failed",
            )
            .with_property("fields", serde_json::to_value(fields).unwrap_or_default()),
            Self::Framework { status, detail } => {
                ProblemDetail::for_status(*status, detail.clone())
            }
            Self::DataIntegrity => ProblemDetail::new(
                StatusCode::CONFLICT,
                "Conflict",
                Some("The request conflicts with existing data.".into()),
                "data-integrity-violation",
            ),
            Self::Internal => ProblemDetail::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error",
                Some("An unexpected error occurred.".into()),
                "internal-error",
            ),
        }
    }

    fn log(&self) {
        match self {
            Self::Internal => tracing::error!("Unhandled exception"),
            Self::DataIntegrity => {} // already logged where it was classified
            _ => tracing::debug!(error = %self, "Request rejected"),
        }
    }
}

fn rule_problem(
    title: &str,
    slug: &str,
    err: &DomainError,
    violations: &[Violation],
) -> ProblemDetail {
    // `detail` is the joined message list, exactly as the Kotlin exception's
    // own `message` was; `violations` is the array the frontend reads off
    // `ApiError.violations` to highlight every problem at once.
    ProblemDetail::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        title,
        Some(err.to_string()),
        slug,
    )
    .with_property(
        "violations",
        serde_json::to_value(violations).unwrap_or_default(),
    )
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        Self::from_sqlx(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.log();
        self.problem().into_response()
    }
}

/// Renders a problem document *and* stashes a copy in the response extensions,
/// so [`crate::http::fill_problem_instance`] can rewrite the body once it knows
/// the request path. A response that somehow escapes that middleware is still a
/// valid problem document, just without `instance`.
impl IntoResponse for ProblemDetail {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = serde_json::to_vec(&self).unwrap_or_else(|_| b"{}".to_vec());
        let mut response = Response::builder()
            .status(status)
            .header(axum::http::header::CONTENT_TYPE, "application/problem+json")
            .body(axum::body::Body::from(body))
            .expect("problem response is well-formed");
        response.extensions_mut().insert(self);
        response
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roster_rule_error_is_a_422_carrying_every_violation() {
        let err = ApiError::Domain(DomainError::RosterRule(vec![
            Violation::new(
                "INCOMPLETE_ROSTER",
                "Roster needs 3 heroes but only 2 selected.",
            ),
            Violation::new("BUDGET_EXCEEDED", "Over by 400."),
        ]));
        let p = err.problem();
        assert_eq!(p.status, 422);
        assert_eq!(
            p.type_uri,
            "https://umfl.dev/problems/roster-rule-violation"
        );
        assert_eq!(
            p.detail.as_deref(),
            Some("Roster needs 3 heroes but only 2 selected.; Over by 400.")
        );
        let violations = p.properties["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0]["rule"], "INCOMPLETE_ROSTER");
    }

    #[test]
    fn the_three_rule_families_keep_distinct_titles_and_slugs() {
        let cases = [
            (
                DomainError::RosterRule(vec![]),
                "Roster rules violated",
                "roster-rule-violation",
            ),
            (
                DomainError::MatchRule(vec![]),
                "Match result rules violated",
                "match-rule-violation",
            ),
            (
                DomainError::ScoringRule(vec![]),
                "Scoring rule set rules violated",
                "scoring-rule-violation",
            ),
        ];
        for (err, title, slug) in cases {
            let p = ApiError::Domain(err).problem();
            assert_eq!(p.title, title);
            assert_eq!(p.type_uri, format!("https://umfl.dev/problems/{slug}"));
        }
    }

    #[test]
    fn domain_statuses_match_the_kotlin_kdoc() {
        assert_eq!(
            ApiError::Domain(DomainError::not_found("x"))
                .problem()
                .status,
            404
        );
        assert_eq!(
            ApiError::Domain(DomainError::conflict("x"))
                .problem()
                .status,
            409
        );
        assert_eq!(
            ApiError::Domain(DomainError::service_unavailable("x"))
                .problem()
                .status,
            503
        );
        assert_eq!(ApiError::Unauthorized.problem().status, 401);
        assert_eq!(ApiError::Forbidden.problem().status, 403);
        assert_eq!(ApiError::DataIntegrity.problem().status, 409);
        assert_eq!(ApiError::Internal.problem().status, 500);
    }

    #[test]
    fn a_validation_failure_names_the_offending_fields() {
        let mut fields = IndexMap::new();
        fields.insert("n".to_owned(), "must be greater than 0".to_owned());
        let p = ApiError::Validation(fields).problem();
        assert_eq!(p.status, 400);
        assert_eq!(p.type_uri, "https://umfl.dev/problems/validation-failed");
        assert_eq!(p.properties["fields"]["n"], "must be greater than 0");
    }
}
