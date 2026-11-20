//! The embedded web UI and its single-page-app fallback.
//!
//! `build.rs` points `PIER_WEB_DIST` at `crates/web/dist` (or a placeholder page when the
//! UI has not been built), and `rust-embed` bakes those files into the binary.

use axum::body::Body;
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$PIER_WEB_DIST"]
struct WebAssets;

const INDEX: &str = "index.html";

fn file_response(path: &str, cache: &'static str) -> Option<Response> {
    let file = WebAssets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut response = Response::new(Body::from(file.data.into_owned()));
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(mime.as_ref()) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    Some(response)
}

/// Serves a static file, or `index.html` for any other non-API path so client-side
/// routes survive a reload.
///
/// `/api/*` never falls through to the SPA: unknown API paths are answered by the API
/// router itself, and this handler refuses them as a second line of defence.
pub async fn serve(method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    // Hashed bundle files can be cached forever; the entry page must always be revalidated.
    let immutable = "public, max-age=31536000, immutable";
    if !path.is_empty() && path != INDEX {
        if let Some(response) = file_response(path, immutable) {
            return response;
        }
        // A request that looks like a file (has an extension) but is missing is a real 404.
        if path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
        {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    file_response(INDEX, "no-cache").unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spa_fallback_serves_index_for_client_routes() {
        let response = serve(Method::GET, "/containers/abc".parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
    }

    #[tokio::test]
    async fn api_paths_never_get_the_spa() {
        for path in ["/api", "/api/", "/api/nope", "/api/containers/x"] {
            let response = serve(Method::GET, path.parse().unwrap()).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[tokio::test]
    async fn missing_files_are_404_and_posts_are_refused() {
        let response = serve(Method::GET, "/missing.js".parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = serve(Method::POST, "/".parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
