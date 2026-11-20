//! Authentication: first-run setup, login, logout and the session middleware.

mod limiter;
pub mod middleware;
pub mod password;
pub mod session;

use std::time::Instant;

use axum::Extension;
use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use pier_api_types::{Credentials, SetupStatus, UserInfo};

pub use limiter::LoginLimiter;
pub use middleware::{CurrentUser, SessionKey};

use crate::api::ApiJson;
use crate::error::ApiError;
use crate::state::AppState;
use crate::store;

/// Longest accepted username.
pub const MAX_USERNAME_LEN: usize = 64;

fn validate_username(username: &str) -> Result<(), ApiError> {
    let ok = !username.is_empty()
        && username.len() <= MAX_USERNAME_LEN
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '@'));
    if ok {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "username must be 1 to 64 characters: letters, digits, '_', '.', '-' or '@'".into(),
        ))
    }
}

fn validate_new_password(password: &str) -> Result<(), ApiError> {
    let len = password.chars().count();
    if (password::MIN_PASSWORD_LEN..=password::MAX_PASSWORD_LEN).contains(&len) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "password must be {} to {} characters",
            password::MIN_PASSWORD_LEN,
            password::MAX_PASSWORD_LEN
        )))
    }
}

/// Creates a session for `user` and returns the response carrying the cookie.
async fn start_session(
    state: &AppState,
    user: &store::User,
    status: StatusCode,
) -> Result<Response, ApiError> {
    let token = session::new_token();
    let now = store::now_secs();
    store::purge_expired_sessions(&state.db, now).await?;
    store::create_session(
        &state.db,
        &session::token_hash(&token),
        user.id,
        now,
        now + state.config.session_ttl_secs,
    )
    .await?;
    let cookie = session::set_cookie(
        &token,
        state.config.session_ttl_secs,
        state.config.cookie_secure,
    )
    .ok_or_else(|| ApiError::Internal("could not build session cookie".into()))?;
    let info = UserInfo {
        id: user.id,
        username: user.username.clone(),
        role: user.role.clone(),
    };
    let mut response = (status, Json(info)).into_response();
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    Ok(response)
}

/// `GET /api/setup/status`: whether the first admin still has to be created.
pub async fn setup_status(State(state): State<AppState>) -> Result<Json<SetupStatus>, ApiError> {
    let users = store::count_users(&state.db).await?;
    Ok(Json(SetupStatus {
        needs_setup: users == 0,
    }))
}

/// `POST /api/setup`: creates the first admin and signs them in.
///
/// Fails with 409 once any user exists, including when two requests race.
pub async fn setup(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<Credentials>,
) -> Result<Response, ApiError> {
    validate_username(&body.username)?;
    validate_new_password(&body.password)?;
    // Cheap early exit; the insert below is the authoritative atomic check.
    if store::count_users(&state.db).await? > 0 {
        return Err(ApiError::SetupAlreadyDone);
    }
    let hash = password::hash(body.password)
        .await
        .map_err(|e| ApiError::Internal(format!("password hashing failed: {e}")))?;
    let user = store::create_first_admin(&state.db, &body.username, &hash, store::now_secs())
        .await?
        .ok_or(ApiError::SetupAlreadyDone)?;
    tracing::info!(user = %user.username, "first admin created");
    start_session(&state, &user, StatusCode::CREATED).await
}

/// `POST /api/auth/login`: verifies credentials and sets the session cookie.
///
/// Unknown users and wrong passwords return the same error after the same amount of
/// hashing work, and repeated failures are locked out by [`LoginLimiter`].
pub async fn login(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<Credentials>,
) -> Result<Response, ApiError> {
    let key = body.username.as_str();
    state
        .limiter
        .check(key, Instant::now())
        .map_err(|retry_after| ApiError::TooManyAttempts { retry_after })?;

    // Oversized input cannot be a valid credential; refuse before hashing anything.
    let plausible = body.username.len() <= MAX_USERNAME_LEN
        && body.password.chars().count() <= password::MAX_PASSWORD_LEN;
    let user = if plausible {
        store::find_user_by_username(&state.db, &body.username).await?
    } else {
        None
    };

    // Unknown users are verified against a dummy hash so both paths cost the same.
    let hash = match &user {
        Some(user) => user.password_hash.clone(),
        None => password::dummy_hash().await.to_string(),
    };
    let matches = plausible
        && password::verify(body.password, hash)
            .await
            .map_err(|e| ApiError::Internal(format!("password verification failed: {e}")))?;

    match user {
        Some(user) if matches => {
            state.limiter.record_success(key);
            start_session(&state, &user, StatusCode::OK).await
        }
        _ => {
            state.limiter.record_failure(key, Instant::now());
            Err(ApiError::InvalidCredentials)
        }
    }
}

/// `POST /api/auth/logout`: deletes the session and clears the cookie.
pub async fn logout(
    State(state): State<AppState>,
    Extension(SessionKey(key)): Extension<SessionKey>,
) -> Result<Response, ApiError> {
    store::delete_session(&state.db, &key).await?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Some(cookie) = session::clear_cookie(state.config.cookie_secure) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

/// `GET /api/auth/me`: the signed-in user.
pub async fn me(Extension(CurrentUser(user)): Extension<CurrentUser>) -> Json<UserInfo> {
    Json(user)
}
