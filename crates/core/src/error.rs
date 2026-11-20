//! The single error type returned by every fallible `pier-core` operation.

use thiserror::Error;

/// Result alias used throughout `pier-core`.
pub type Result<T, E = CoreError> = std::result::Result<T, E>;

/// Alias so call sites can write `pier_core::Error` as `style.mdc` expects.
pub type Error = CoreError;

/// Domain errors raised by the Docker wrapper and the input validators.
///
/// The server maps each variant to one HTTP status code in a single place; the
/// variants are therefore coarse and describe *what the caller can do about it*,
/// not which Docker endpoint failed.
#[derive(Debug, Error)]
pub enum CoreError {
    /// The caller supplied a value Docker would reject or that we refuse to forward.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// The addressed network, volume, container or image does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// The operation conflicts with the resource state (in use, already exists, running).
    #[error("conflict: {0}")]
    Conflict(String),
    /// Docker refuses the operation outright (for example removing a predefined network).
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// The Docker daemon cannot be reached (socket missing, refused, timed out).
    #[error("docker daemon unavailable: {0}")]
    Unavailable(String),
    /// The Docker connection settings are malformed or unsupported on this platform.
    #[error("invalid docker connection configuration: {0}")]
    InvalidConfig(String),
    /// Docker answered, but with an error we cannot classify further.
    #[error("docker error: {0}")]
    Docker(String),
}

impl From<bollard::errors::Error> for CoreError {
    fn from(err: bollard::errors::Error) -> Self {
        use bollard::errors::Error as B;
        match err {
            B::DockerResponseServerError {
                status_code,
                message,
            } => Self::from_status(status_code, message),
            // Anything that is not an HTTP answer from the daemon is a transport problem:
            // missing socket, refused connection, timeout, TLS or SSH failure.
            B::JsonDataError { .. }
            | B::JsonSerdeError { .. }
            | B::DockerStreamError { .. }
            | B::DockerContainerWaitError { .. } => Self::Docker(err.to_string()),
            other => Self::Unavailable(other.to_string()),
        }
    }
}

impl CoreError {
    /// Classifies an HTTP status returned by the Docker daemon.
    #[must_use]
    pub fn from_status(status: u16, message: String) -> Self {
        match status {
            400 => Self::InvalidInput(message),
            403 => Self::Forbidden(message),
            404 => Self::NotFound(message),
            409 => Self::Conflict(message),
            _ => Self::Docker(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_are_classified() {
        assert!(matches!(
            CoreError::from_status(404, "x".into()),
            CoreError::NotFound(_)
        ));
        assert!(matches!(
            CoreError::from_status(409, "x".into()),
            CoreError::Conflict(_)
        ));
        assert!(matches!(
            CoreError::from_status(403, "x".into()),
            CoreError::Forbidden(_)
        ));
        assert!(matches!(
            CoreError::from_status(400, "x".into()),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            CoreError::from_status(500, "x".into()),
            CoreError::Docker(_)
        ));
    }

    #[test]
    fn transport_errors_are_unavailable() {
        let err = bollard::errors::Error::SocketNotFoundError("/nope".into());
        assert!(matches!(CoreError::from(err), CoreError::Unavailable(_)));
        let err = bollard::errors::Error::RequestTimeoutError;
        assert!(matches!(CoreError::from(err), CoreError::Unavailable(_)));
    }
}
