//! The HTTP plumbing that reproduces what Spring MVC did for free: problem
//! documents for the framework's own rejections, and the `instance` field the
//! servlet layer filled in on its way out.

pub mod extract;
pub mod problem;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;
use problem::ProblemDetail;

/// Fills a problem document's `instance` with the request path.
///
/// Spring did this in `RequestResponseBodyMethodProcessor`: any `ProblemDetail`
/// returned from a handler with a null `instance` got
/// `HttpServletRequest.getRequestURI()` written into it just before
/// serialisation. Every error body this API has ever produced therefore carries
/// the path, and dropping it would be a wire change.
///
/// `IntoResponse for ProblemDetail` leaves a clone in the response extensions
/// for exactly this. Placed outside `CatchPanicLayer` so the 500 a panic
/// produces gets an `instance` too.
pub async fn fill_problem_instance(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    let mut response = next.run(req).await;

    let Some(problem) = response.extensions_mut().remove::<ProblemDetail>() else {
        return response;
    };
    if problem.instance.is_some() {
        return response;
    }

    let problem = ProblemDetail {
        instance: Some(path),
        ..problem
    };
    let body = serde_json::to_vec(&problem).unwrap_or_else(|_| b"{}".to_vec());
    let (mut parts, _) = response.into_parts();
    // The body just grew; a stale Content-Length would truncate it.
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(body))
}

/// `NoResourceFoundException` -- a path matching no route at all.
///
/// Note this is the *routing* fallback, not the authorization one: a request
/// for an unrouted path under `/api/**` is denied by the rule table before it
/// ever reaches here, which is `anyRequest().denyAll()`'s whole job and the
/// reason authorization is one middleware rather than a per-route layer.
pub async fn not_found_fallback(uri: Uri) -> ApiError {
    ApiError::Framework {
        status: StatusCode::NOT_FOUND,
        detail: format!("No static resource {}.", uri.path().trim_start_matches('/')),
    }
}

/// `HttpRequestMethodNotSupportedException` -- a known path, wrong verb.
pub async fn method_not_allowed_fallback(method: Method) -> ApiError {
    ApiError::Framework {
        status: StatusCode::METHOD_NOT_ALLOWED,
        detail: format!("Method '{method}' is not supported."),
    }
}

/// What `CatchPanicLayer` renders. Stands in for `handleUnexpected`'s 500 --
/// a panic is this runtime's equivalent of the exception nobody anticipated.
pub fn panic_response(err: Box<dyn std::any::Any + Send + 'static>) -> Response {
    let detail = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("unknown panic");
    tracing::error!(panic = detail, "Handler panicked");
    ApiError::Internal.into_response()
}
