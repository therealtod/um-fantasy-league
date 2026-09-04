//! Extractors whose rejections are RFC 7807 problem documents.
//!
//! **Never use a bare `axum::Json`, `Path` or `Query` in a handler.** A bare
//! one rejects with axum's plain-text body and no problem type, which is a wire
//! change. That rule is grep-able on purpose.
//!
//! The `detail` strings below are pinned wire contract, asserted exactly by
//! the integration suite -- treat a wording change here as a breaking change,
//! not a copyedit.

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
        // Captured before the body is consumed: the 415 below names the
        // offending Content-Type.
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
            // body that could not be read at all -- collapses to this one
            // sentence, which says exactly this much and no more.
            Err(_) => Err(ApiError::Framework {
                status: StatusCode::BAD_REQUEST,
                detail: "Failed to read request".to_owned(),
            }),
        }
    }
}

/// A JSON body that must also satisfy its `garde` rules.
///
/// The 400 it raises carries a `fields` object mapping each failing path to
/// its message, so the client can highlight every bad field at once rather
/// than one per round trip. garde renders a path as `a.b[0].c`.
///
/// The one adjustment is [`camel_case`]: garde names the Rust field, which is
/// snake_case, but every request DTO here is
/// `#[serde(rename_all = "camelCase")]` on the wire. So the mapping is total
/// and belongs at this single boundary rather than being spelled out per
/// field — garde 0.23 has no `rename` attribute to spell
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
/// Only `_x` becomes `X`; the `.` and `[0]` separators garde emits already
/// match the wire's field-path shape, and a leading underscore is left alone
/// because it is not a word boundary in any DTO here.
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
        // all -- only the multi-parameter form reports one. Naming the
        // variable in both cases needs the name recovered from the route's
        // own parameter list here.
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
/// This used to answer 500 before the base class went back in -- an
/// unparseable path variable now gets this message instead.
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
            // it is what keeps `/api/tournaments/{id}` naming the field
            // rather than falling back to axum's generic sentence.
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
/// **Known limitation.** [`AppPath`] names the offending parameter
/// (`Failed to convert 'sinceMatchId' with value: 'abc'`); this does not.
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
    /// the camelCase one `types.ts` declares.
    #[test]
    fn a_field_path_is_reported_in_the_names_the_wire_uses() {
        assert_eq!(camel_case("hero_ids"), "heroIds");
        assert_eq!(camel_case("roster_size"), "rosterSize");
        assert_eq!(camel_case("games[0].map_id"), "games[0].mapId");
        assert_eq!(camel_case("name"), "name");
    }
}
