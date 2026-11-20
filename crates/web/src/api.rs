//! Thin fetch wrapper around the Pier REST API.
//!
//! Every call sends `Content-Type: application/json` (the server's CSRF guard requires it
//! on state-changing requests) and the browser attaches the `HttpOnly` session cookie.
//! A `401` from any endpoint other than login/setup sends the user to the login page.

use gloo_net::http::{Method, Request, RequestBuilder, Response};
use pier_api_types::ErrorBody;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A failed API call, ready to show to the user.
#[derive(Debug, Clone)]
pub struct ApiFailure {
    /// Message to display.
    pub message: String,
}

impl std::fmt::Display for ApiFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Result of an API call.
pub type ApiResult<T> = Result<T, ApiFailure>;

fn network_error(err: &gloo_net::Error) -> ApiFailure {
    ApiFailure {
        message: format!("cannot reach the Pier server: {err}"),
    }
}

/// Paths where a 401 means "wrong credentials" rather than "session expired".
fn is_auth_entry(path: &str) -> bool {
    matches!(path, "/api/auth/login" | "/api/setup" | "/api/setup/status")
}

fn redirect_to_login() {
    if let Some(window) = web_sys::window() {
        let _ = window.location().set_href("/login");
    }
}

fn builder(method: Method, path: &str) -> RequestBuilder {
    Request::get(path)
        .method(method)
        .header("Content-Type", "application/json")
}

async fn failure(path: &str, response: Response) -> ApiFailure {
    let status = response.status();
    if status == 401 && !is_auth_entry(path) {
        redirect_to_login();
    }
    let message = match response.json::<ErrorBody>().await {
        Ok(body) => body.error,
        Err(_) => format!("request failed with status {status}"),
    };
    ApiFailure { message }
}

async fn finish(path: &str, response: Response) -> ApiResult<Response> {
    if response.ok() {
        Ok(response)
    } else {
        Err(failure(path, response).await)
    }
}

async fn decode<T: DeserializeOwned>(path: &str, response: Response) -> ApiResult<T> {
    let response = finish(path, response).await?;
    response.json::<T>().await.map_err(|err| ApiFailure {
        message: format!("unexpected response: {err}"),
    })
}

/// `GET path`, decoding the JSON response.
pub async fn get<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    let response = builder(Method::GET, path)
        .send()
        .await
        .map_err(|e| network_error(&e))?;
    decode(path, response).await
}

/// `POST path` with a JSON body, decoding the JSON response.
pub async fn post<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    let request = builder(Method::POST, path)
        .json(body)
        .map_err(|e| network_error(&e))?;
    let response = request.send().await.map_err(|e| network_error(&e))?;
    decode(path, response).await
}

/// `POST path` with an empty JSON object, decoding the JSON response.
pub async fn post_empty<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    post(path, &serde_json::json!({})).await
}

/// `POST path` with an empty JSON object, ignoring the (empty) response body.
pub async fn action(path: &str) -> ApiResult<()> {
    let request = builder(Method::POST, path)
        .json(&serde_json::json!({}))
        .map_err(|e| network_error(&e))?;
    let response = request.send().await.map_err(|e| network_error(&e))?;
    finish(path, response).await.map(|_| ())
}

/// `DELETE path`.
pub async fn delete(path: &str) -> ApiResult<()> {
    let response = builder(Method::DELETE, path)
        .send()
        .await
        .map_err(|e| network_error(&e))?;
    finish(path, response).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_login_and_setup_keep_a_401_on_the_page() {
        assert!(is_auth_entry("/api/auth/login"));
        assert!(is_auth_entry("/api/setup"));
        assert!(!is_auth_entry("/api/containers"));
        assert!(!is_auth_entry("/api/auth/me"));
    }
}
