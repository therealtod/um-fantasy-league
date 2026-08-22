//! Extractors whose rejections are RFC 7807 problem documents.
//!
//! Oracle: `ResponseEntityExceptionHandler` -- the base class
//! `GlobalExceptionHandler` inherits from purely so that Spring MVC's own
//! failures answer with their real status instead of the catch-all's 500. axum
//! has no such resolver ordering, so the equivalent coverage has to come from
//! the extractors themselves.
//!
//! **Never use a bare `axum::Json`, `Path` or `Query` in a handler.** A bare
//! one rejects with axum's plain-text body and no problem type, which is a wire
//! change. That rule is grep-able on purpose -- see `PORTING.md`.
//!
//! The `detail` strings are Spring's own, read out of spring-web 7.0.8 rather
//! than paraphrased.

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{FromRequest, FromRequestParts, Path, Query, RawPathParams, Request};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use indexmap::IndexMap;
use serde::de::DeserializeOwned;

use crate::error::ApiError;

/// A JSON body. Replaces `axum::Json` on the request side.
#[derive(Debug, Clone, Copy, Default)]
pub struct AppJson<T>(pub T);

impl<S, T> FromRequest<S> for AppJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Captured before the body is consumed: the 415 names the offending
        // Content-Type, as `HttpMediaTypeNotSupportedException` does.
        let content_type = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(JsonRejection::MissingJsonContentType(_)) => Err(ApiError::Framework {
                status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
                detail: match content_type {
                    Some(ct) => format!("Content-Type '{ct}' is not supported."),
                    None => "Content-Type is not supported.".to_owned(),
                },
            }),
            // Everything else -- unparseable JSON, a value of the wrong type, a
            // body that could not be read at all -- is what Spring surfaced as
            // `HttpMessageNotReadableException`, and it says exactly this much.
            Err(_) => Err(ApiError::Framework {
                status: StatusCode::BAD_REQUEST,
                detail: "Failed to read request".to_owned(),
            }),
        }
    }
}

/// A JSON body that must also satisfy its `garde` rules.
///
/// The 400 it raises is `handleMethodArgumentNotValid`'s: a `fields` object
/// mapping each failing path to its message, so the client can highlight every
/// bad field at once rather than one per round trip. garde renders a path as
/// `a.b[0].c`, which is the shape Spring's `FieldError.getField()` produced.
///
/// The one adjustment is [`camel_case`]: `FieldError.getField()` named the
/// *Java* field, which is camelCase, while garde names the Rust one, which is
/// snake_case. Every request DTO here is `#[serde(rename_all = "camelCase")]`,
/// so the mapping is total and belongs at this single boundary rather than
/// being spelled out per field — garde 0.23 has no `rename` attribute to spell
/// it with anyway.
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidJson<T>
where
    T: DeserializeOwned + garde::Validate<Context = ()>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let AppJson(value) = AppJson::<T>::from_request(req, state).await?;
        if let Err(report) = value.validate() {
            let fields: IndexMap<String, String> = report
                .iter()
                .map(|(path, error)| (camel_case(&path.to_string()), error.to_string()))
                .collect();
            return Err(ApiError::Validation(fields));
        }
        Ok(Self(value))
    }
}

/// Re-spells a garde path in the field names the wire uses.
///
/// Only `_x` becomes `X`; the `.` and `[0]` separators garde emits are already
/// what Spring's `FieldError.getField()` produced, and a leading underscore is
/// left alone because it is not a word boundary in any DTO here.
fn camel_case(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut upper_next = false;
    for c in path.chars() {
        if c == '_' && !out.is_empty() && !out.ends_with(['.', '[']) {
            upper_next = true;
        } else if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Path variables. Replaces `axum::extract::Path`.
#[derive(Debug, Clone, Copy, Default)]
pub struct AppPath<T>(pub T);

impl<S, T> FromRequestParts<S> for AppPath<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Captured before delegating, because a **single**-parameter `Path<T>`
        // deserializes the value directly and its rejection carries no key at
        // all -- only the multi-parameter form reports one. Spring named the
        // variable in both cases, so the name is recovered from the route's own
        // parameter list here.
        let names: Vec<String> = RawPathParams::from_request_parts(parts, state)
            .await
            .map(|params| {
                params
                    .iter()
                    .map(|(name, _)| name.to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        match Path::<T>::from_request_parts(parts, state).await {
            Ok(Path(value)) => Ok(Self(value)),
            Err(rejection) => Err(path_error(rejection, &names)),
        }
    }
}

/// `MethodArgumentTypeMismatchException`, as `handleTypeMismatch` renders it:
/// `Failed to convert 'id' with value: 'not-a-number'`.
///
/// This is the defect `GlobalExceptionHandlerMvcTest` was written for -- an
/// unparseable path variable answered 500 before the base class went back in.
fn path_error(rejection: PathRejection, names: &[String]) -> ApiError {
    use axum::extract::path::ErrorKind;

    let detail = match rejection {
        PathRejection::FailedToDeserializePathParams(err) => match err.into_kind() {
            ErrorKind::ParseErrorAtKey { key, value, .. }
            | ErrorKind::DeserializeError { key, value, .. } => {
                format!("Failed to convert '{key}' with value: '{value}'")
            }
            // The single-parameter form, which knows the value but not the
            // name. There is exactly one name to reach for, and reaching for
            // it is what keeps `/api/tournaments/{id}` answering Spring's
            // sentence rather than axum's.
            ErrorKind::ParseError { value, .. } => match names {
                [only] => format!("Failed to convert '{only}' with value: '{value}'"),
                _ => format!("Failed to convert path variable with value: '{value}'"),
            },
            other => format!("Failed to convert path variable: {other}"),
        },
        other => other.body_text(),
    };
    ApiError::Framework {
        status: StatusCode::BAD_REQUEST,
        detail,
    }
}

/// Query parameters. Replaces `axum::extract::Query`.
///
/// **Known deviation, allowlisted in the differential rig.** Spring named the
/// offending parameter (`Failed to convert 'sinceMatchId' with value: 'abc'`);
/// `serde_urlencoded` reports only what went wrong, not which key it was
/// deserialising, and recovering the key would mean deserialising field by
/// field through a hand-written `Deserializer`. Status (400) and problem type
/// (`bad-request`) match; only the `detail` sentence differs, and only for a
/// request that was already malformed.
#[derive(Debug, Clone, Copy, Default)]
pub struct AppQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for AppQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError::Framework {
                status: StatusCode::BAD_REQUEST,
                detail: rejection.body_text(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::camel_case;

    /// `fields` is keyed by the name the client knows the field by, which is
    /// the camelCase one Jackson emitted and `types.ts` declares.
    #[test]
    fn a_field_path_is_reported_in_the_names_the_wire_uses() {
        assert_eq!(camel_case("hero_ids"), "heroIds");
        assert_eq!(camel_case("roster_size"), "rosterSize");
        assert_eq!(camel_case("games[0].map_id"), "games[0].mapId");
        assert_eq!(camel_case("name"), "name");
    }
}
