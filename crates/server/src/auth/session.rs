//! Session tokens and the session cookie.
//!
//! The browser holds a random 256-bit token; the database stores only its SHA-256, so a
//! database leak cannot be turned back into valid cookies.

use axum::http::{HeaderMap, HeaderValue, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Name of the session cookie.
pub const COOKIE_NAME: &str = "pier_session";

/// Generates a fresh random session token (URL-safe base64, 43 characters).
#[must_use]
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Returns the storage key for a token: lowercase hex of its SHA-256.
#[must_use]
pub fn token_hash(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Extracts the session token from the `Cookie` header, if present and well-formed.
#[must_use]
pub fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.to_string())
        .filter(|value| !value.is_empty() && value.len() <= 128)
}

fn cookie(value: &str, max_age: i64, secure: bool) -> Option<HeaderValue> {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Strict{secure}"
    ))
    .ok()
}

/// Builds the `Set-Cookie` value that starts a session.
#[must_use]
pub fn set_cookie(token: &str, ttl_secs: i64, secure: bool) -> Option<HeaderValue> {
    cookie(token, ttl_secs, secure)
}

/// Builds the `Set-Cookie` value that clears the session cookie.
#[must_use]
pub fn clear_cookie(secure: bool) -> Option<HeaderValue> {
    cookie("", 0, secure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_hash_is_stable() {
        let a = new_token();
        let b = new_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert_eq!(token_hash(&a), token_hash(&a));
        assert_eq!(token_hash(&a).len(), 64);
        assert_ne!(token_hash(&a), a);
    }

    #[test]
    fn cookie_has_security_attributes() {
        let value = set_cookie("abc", 60, false).unwrap();
        let text = value.to_str().unwrap();
        assert!(text.contains("HttpOnly"));
        assert!(text.contains("SameSite=Strict"));
        assert!(text.contains("Path=/"));
        assert!(!text.contains("Secure"));
        assert!(
            set_cookie("abc", 60, true)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("; Secure")
        );
    }

    #[test]
    fn parses_the_cookie_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; pier_session=tok123; b=2"),
        );
        assert_eq!(token_from_headers(&headers).as_deref(), Some("tok123"));
        headers.insert(header::COOKIE, HeaderValue::from_static("other=1"));
        assert_eq!(token_from_headers(&headers), None);
        headers.insert(header::COOKIE, HeaderValue::from_static("pier_session="));
        assert_eq!(token_from_headers(&headers), None);
    }
}
