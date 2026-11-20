//! HTTP error type and the one place where domain errors become status codes.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use pier_api_types::ErrorBody;
use pier_core::CoreError;

/// Every error a handler or middleware can return.
#[derive(Debug)]
pub enum ApiError {
    /// A Docker or validation error from `pier-core`.
    Core(CoreError),
    /// No valid session cookie.
    Unauthorized,
    /// Wrong username or password (deliberately the same message for both).
    InvalidCredentials,
    /// Too many failed logins; retry after the given number of seconds.
    TooManyAttempts {
        /// Seconds until another attempt is allowed.
        retry_after: u64,
    },
    /// First-run setup was already completed.
    SetupAlreadyDone,
    /// A request field failed validation.
    BadRequest(String),
    /// A request rejected by an extractor or guard, with its own status.
    Rejected(StatusCode, String),
    /// No such route.
    NotFound,
    /// Anything unexpected; details are logged, not returned.
    Internal(String),
}

/// Maps a [`CoreError`] to an HTTP status. This is the only such mapping in the server.
#[must_use]
pub fn core_status(err: &CoreError) -> StatusCode {
    match err {
        CoreError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        CoreError::NotFound(_) => StatusCode::NOT_FOUND,
        CoreError::Conflict(_) => StatusCode::CONFLICT,
        CoreError::Forbidden(_) => StatusCode::FORBIDDEN,
        CoreError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        CoreError::InvalidConfig(_) => StatusCode::INTERNAL_SERVER_ERROR,
        CoreError::Docker(_) => StatusCode::BAD_GATEWAY,
    }
}

impl From<CoreError> for ApiError {
    fn from(err: CoreError) -> Self {
        Self::Core(err)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        Self::Internal(format!("database error: {err}"))
    }
}

fn core_message(err: &CoreError) -> String {
    match err {
        // Connection details (socket paths, hosts) are logged but not sent to the client.
        CoreError::Unavailable(_) => "docker daemon is unreachable".to_string(),
        CoreError::InvalidConfig(_) => "docker connection is misconfigured".to_string(),
        CoreError::InvalidInput(m)
        | CoreError::NotFound(m)
        | CoreError::Conflict(m)
        | CoreError::Forbidden(m)
        | CoreError::Docker(m) => m.to_lowercase(),
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message, retry_after) = match &self {
            Self::Core(err) => {
                let status = core_status(err);
                if status.is_server_error() {
                    tracing::warn!(error = %err, %status, "docker request failed");
                }
                (status, core_message(err), None)
            }
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "authentication required".into(),
                None,
            ),
            Self::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "invalid username or password".into(),
                None,
            ),
            Self::TooManyAttempts { retry_after } => (
                StatusCode::TOO_MANY_REQUESTS,
                "too many failed login attempts, try again later".into(),
                Some(*retry_after),
            ),
            Self::SetupAlreadyDone => (
                StatusCode::CONFLICT,
                "setup has already been completed".into(),
                None,
            ),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message.clone(), None),
            Self::Rejected(status, message) => (*status, message.clone(), None),
            Self::NotFound => (StatusCode::NOT_FOUND, "not found".into(), None),
            Self::Internal(detail) => {
                tracing::error!(%detail, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".into(),
                    None,
                )
            }
        };
        let mut response = (status, Json(ErrorBody { error: message })).into_response();
        if let Some(seconds) = retry_after {
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_core_error_has_the_expected_status() {
        let cases = [
            (CoreError::InvalidInput("x".into()), StatusCode::BAD_REQUEST),
            (CoreError::NotFound("x".into()), StatusCode::NOT_FOUND),
            (CoreError::Conflict("x".into()), StatusCode::CONFLICT),
            (CoreError::Forbidden("x".into()), StatusCode::FORBIDDEN),
            (
                CoreError::Unavailable("x".into()),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                CoreError::InvalidConfig("x".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (CoreError::Docker("x".into()), StatusCode::BAD_GATEWAY),
        ];
        for (err, status) in cases {
            assert_eq!(core_status(&err), status, "{err}");
        }
    }

    #[test]
    fn unavailable_hides_connection_details() {
        let msg = core_message(&CoreError::Unavailable(
            "socket /var/run/docker.sock refused".into(),
        ));
        assert!(!msg.contains("docker.sock"));
    }

    #[test]
    fn rate_limit_sets_retry_after() {
        let response = ApiError::TooManyAttempts { retry_after: 30 }.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "30");
    }
}
