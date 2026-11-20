//! Request guards: session authentication, CSRF protection and response hardening.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use pier_api_types::UserInfo;

use super::session;
use crate::error::ApiError;
use crate::state::AppState;
use crate::store;

/// The authenticated user, inserted into request extensions by [`require_auth`].
#[derive(Debug, Clone)]
pub struct CurrentUser(pub UserInfo);

/// Rejects requests without a valid, unexpired session cookie (401).
///
/// On success the [`CurrentUser`] and the session's token hash are added to the request
/// extensions.
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = session::token_from_headers(req.headers()).ok_or(ApiError::Unauthorized)?;
    let hash = session::token_hash(&token);
    let user = store::find_session_user(&state.db, &hash, store::now_secs())
        .await?
        .ok_or(ApiError::Unauthorized)?;
    req.extensions_mut().insert(CurrentUser(UserInfo {
        id: user.id,
        username: user.username,
        role: user.role,
    }));
    req.extensions_mut().insert(SessionKey(hash));
    Ok(next.run(req).await)
}

/// Storage key of the current session, used by logout.
#[derive(Debug, Clone)]
pub struct SessionKey(pub String);

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            let essence = v.split(';').next().unwrap_or("").trim();
            essence.eq_ignore_ascii_case("application/json")
        })
}

fn origin_matches_host(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    origin
        .to_str()
        .ok()
        .and_then(|o| o.split_once("://"))
        .is_some_and(|(_, authority)| authority.eq_ignore_ascii_case(host))
}

/// CSRF guard for the API.
///
/// State-changing requests must declare `Content-Type: application/json` (a cross-site
/// HTML form cannot send that without a CORS preflight, which Pier never grants), and an
/// `Origin` header, when present, must match the `Host`. The session cookie is also
/// `SameSite=Strict`.
pub async fn csrf_guard(req: Request, next: Next) -> Result<Response, ApiError> {
    let unsafe_method = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if unsafe_method {
        if !origin_matches_host(req.headers()) {
            return Err(ApiError::Rejected(
                StatusCode::FORBIDDEN,
                "cross-origin request refused".into(),
            ));
        }
        if !is_json(req.headers()) {
            return Err(ApiError::Rejected(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "content-type must be application/json".into(),
            ));
        }
    }
    Ok(next.run(req).await)
}

/// Adds security headers to every response.
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self' 'wasm-unsafe-eval' 'unsafe-inline'; \
             style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; \
             frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
    ] {
        headers.insert(
            header::HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                header::HeaderName::from_static(k),
                HeaderValue::from_static(v),
            );
        }
        map
    }

    #[test]
    fn json_content_type_detection() {
        assert!(is_json(&headers(&[("content-type", "application/json")])));
        assert!(is_json(&headers(&[(
            "content-type",
            "Application/JSON; charset=utf-8"
        )])));
        assert!(!is_json(&headers(&[(
            "content-type",
            "application/x-www-form-urlencoded"
        )])));
        assert!(!is_json(&headers(&[("content-type", "text/plain")])));
        assert!(!is_json(&headers(&[])));
    }

    #[test]
    fn origin_must_match_host() {
        assert!(origin_matches_host(&headers(&[])));
        assert!(origin_matches_host(&headers(&[
            ("host", "localhost:8640"),
            ("origin", "http://localhost:8640")
        ])));
        assert!(!origin_matches_host(&headers(&[
            ("host", "localhost:8640"),
            ("origin", "http://evil.example")
        ])));
        assert!(!origin_matches_host(&headers(&[
            ("host", "localhost:8640"),
            ("origin", "null")
        ])));
        assert!(!origin_matches_host(&headers(&[(
            "origin",
            "http://localhost:8640"
        )])));
    }
}
