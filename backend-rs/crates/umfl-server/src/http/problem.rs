//! RFC 7807 problem documents, shaped exactly as Spring's `ProblemDetail`
//! serialises them.
//!
//! Oracle: `common/GlobalExceptionHandler.kt` plus Spring Framework 7.0.8's
//! `ProblemDetail` + `ProblemDetailJacksonMixin`. Three details of that
//! serialisation are contract and are reproduced here deliberately:
//!
//! * **Field order** is `type, title, status, detail, instance`, then any
//!   extra properties — Jackson's declaration order over `ProblemDetail`'s
//!   fields, with `properties` an `@JsonAnyGetter` that flattens last.
//! * **`@JsonInclude(NON_EMPTY)`** means a null `detail` is *absent*, not
//!   `null` — the same rule as the app-wide `default-property-inclusion:
//!   non_null`. Every `Option` here therefore carries `skip_serializing_if`.
//! * **`instance` is filled in by the framework, not by the handler.**
//!   `RequestResponseBodyMethodProcessor` sets it to `HttpServletRequest
//!   .getRequestURI()` whenever a returned `ProblemDetail` leaves it null, so
//!   every error body this API has ever produced carries the request path.
//!   [`super::fill_problem_instance`] is where that happens on this side.

use axum::http::StatusCode;
use indexmap::IndexMap;
use serde::Serialize;

/// The problem-type vocabulary. Every error this API returns names one.
pub const PROBLEM_TYPE_PREFIX: &str = "https://umfl.dev/problems/";

/// An RFC 7807 problem document.
///
/// Also travels in the response's extensions so the instance-filling
/// middleware can rewrite the body once it knows the request path; that is
/// why this derives `Clone`.
#[derive(Debug, Clone, Serialize)]
pub struct ProblemDetail {
    #[serde(rename = "type")]
    pub type_uri: String,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// `setProperty(...)` on the Kotlin side: the `violations` array on a 422
    /// and the `fields` object on a validation 400. Flattened into the
    /// document root, exactly as `@JsonAnyGetter` does.
    #[serde(flatten)]
    pub properties: IndexMap<String, serde_json::Value>,
}

impl ProblemDetail {
    /// The hand-written form: `GlobalExceptionHandler.problem(status, title, detail, slug)`.
    pub fn new(
        status: StatusCode,
        title: impl Into<String>,
        detail: Option<String>,
        slug: &str,
    ) -> Self {
        Self {
            type_uri: format!("{PROBLEM_TYPE_PREFIX}{slug}"),
            title: title.into(),
            status: status.as_u16(),
            detail,
            instance: None,
            properties: IndexMap::new(),
        }
    }

    /// The framework form: `handleExceptionInternal`'s fallback, where the
    /// title is the status' reason phrase and the slug is that same phrase
    /// lowercased with spaces hyphenated (`Method Not Allowed` ->
    /// `method-not-allowed`).
    pub fn for_status(status: StatusCode, detail: impl Into<String>) -> Self {
        let reason = status.canonical_reason().unwrap_or("Error");
        let slug = reason.to_lowercase().replace(' ', "-");
        Self::new(status, reason, Some(detail.into()), &slug)
    }

    /// `ProblemDetail.setProperty(name, value)`.
    pub fn with_property(mut self, name: &str, value: serde_json::Value) -> Self {
        self.properties.insert(name.to_owned(), value);
        self
    }

    pub fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// Renders this document **without** leaving a copy in the response
    /// extensions, so [`super::fill_problem_instance`] does not add `instance`.
    ///
    /// This is the security-filter half of a split the Kotlin makes by
    /// accident and the wire therefore depends on. `instance` is filled in by
    /// `RequestResponseBodyMethodProcessor`, which is Spring **MVC** -- it only
    /// ever sees a document a *handler* returned. The three rejections raised
    /// inside the filter chain (`ProblemDetailAuthenticationEntryPoint`'s 401,
    /// `ProblemDetailAccessDeniedHandler`'s 403 and `RateLimitFilter`'s 429)
    /// serialise themselves straight to the response with `jsonMapper
    /// .writeValue`, never reach `DispatcherServlet`, and so carry no
    /// `instance` at all. Verified against the running Kotlin backend:
    ///
    /// ```text
    /// GET /api/me            -> {"detail":"Authentication is required...","status":401,...}   (no instance)
    /// GET /api/tournaments/999 -> {"detail":"No tournament with id 999","instance":"/api/tournaments/999",...}
    /// ```
    ///
    /// Emitting `instance` on a 401 would be an added field, which is as much
    /// a contract break as a missing one.
    pub fn into_response_without_instance(self) -> axum::response::Response {
        let status = self.status_code();
        let body = serde_json::to_vec(&self).unwrap_or_else(|_| b"{}".to_vec());
        axum::response::Response::builder()
            .status(status)
            .header(axum::http::header::CONTENT_TYPE, "application/problem+json")
            .body(axum::body::Body::from(body))
            .expect("problem response is well-formed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_spring_field_order_and_omits_absent_fields() {
        let p = ProblemDetail::new(
            StatusCode::NOT_FOUND,
            "Not found",
            Some("No tournament with id 99".into()),
            "not-found",
        );
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"type":"https://umfl.dev/problems/not-found","title":"Not found","status":404,"detail":"No tournament with id 99"}"#
        );
    }

    #[test]
    fn flattens_properties_after_the_declared_fields() {
        let p = ProblemDetail::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Roster rules violated",
            Some("boom".into()),
            "roster-rule-violation",
        )
        .with_property(
            "violations",
            serde_json::json!([{"rule": "BUDGET_EXCEEDED", "message": "boom"}]),
        );
        let s = serde_json::to_string(&p).unwrap();
        assert!(
            s.ends_with(r#""violations":[{"rule":"BUDGET_EXCEEDED","message":"boom"}]}"#),
            "{s}"
        );
    }

    #[test]
    fn derives_the_framework_slug_from_the_reason_phrase() {
        // `handleExceptionInternal`'s fallback: 405 -> `method-not-allowed`.
        let p = ProblemDetail::for_status(
            StatusCode::METHOD_NOT_ALLOWED,
            "Method 'DELETE' is not supported.",
        );
        assert_eq!(p.type_uri, "https://umfl.dev/problems/method-not-allowed");
        assert_eq!(p.title, "Method Not Allowed");
    }

    #[test]
    fn a_null_detail_is_absent_rather_than_null() {
        let document: serde_json::Value = serde_json::to_value(ProblemDetail::new(
            StatusCode::FORBIDDEN,
            "Forbidden",
            None,
            "forbidden",
        ))
        .unwrap();

        // Asserted against the parsed document rather than the rendered string.
        // A `!contains("null")` reads the same and is not the same: it fires on
        // any *string value* carrying those four letters -- a `detail` sentence
        // mentioning a null, a slug like `null-hero` -- and it cannot tell an
        // absent key from one present and null, which is the whole distinction
        // under test. `lib.rs` walks a whole response tree this way; this
        // document is flat, so the two assertions below say all of it.
        let object = document.as_object().expect("a problem detail is an object");
        assert!(!object.contains_key("detail"), "{document}");
        assert!(
            object.values().all(|v| !v.is_null()),
            "a `non_null` document rendered a JSON null: {document}"
        );
    }
}
