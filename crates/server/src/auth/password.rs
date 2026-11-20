//! Argon2id password hashing.
//!
//! Hashing and verification are CPU-heavy by design, so callers run them through
//! [`hash`] / [`verify`], which move the work onto tokio's blocking pool.

use std::sync::OnceLock;

use argon2::password_hash::PasswordVerifier;
use argon2::password_hash::phc::PasswordHash;
use argon2::{Argon2, PasswordHasher};

/// Shortest accepted password.
pub const MIN_PASSWORD_LEN: usize = 8;
/// Longest accepted password; bounds the work an attacker can force per request.
pub const MAX_PASSWORD_LEN: usize = 256;

fn hasher() -> Argon2<'static> {
    Argon2::default()
}

fn hash_blocking(password: &str) -> Result<String, String> {
    hasher()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

/// Hashes a password into a PHC string with a fresh random salt.
///
/// # Errors
/// Returns a message when hashing fails or the blocking task panicked.
pub async fn hash(password: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || hash_blocking(&password))
        .await
        .map_err(|e| e.to_string())?
}

/// Verifies a password against a PHC string. A malformed hash counts as a mismatch.
///
/// # Errors
/// Returns a message only when the blocking task panicked.
pub async fn verify(password: String, phc: String) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        let Ok(parsed) = PasswordHash::new(&phc) else {
            return false;
        };
        hasher()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
    .await
    .map_err(|e| e.to_string())
}

/// A valid hash of a random password, verified when the username is unknown so that
/// "no such user" costs the same as "wrong password".
pub async fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    if let Some(existing) = DUMMY.get() {
        return existing;
    }
    let computed = hash("pier-dummy-password-for-timing".to_string())
        .await
        .unwrap_or_default();
    DUMMY.get_or_init(|| computed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_then_verify() {
        let phc = hash("correct horse".into()).await.unwrap();
        assert!(phc.starts_with("$argon2id$"));
        assert!(verify("correct horse".into(), phc.clone()).await.unwrap());
        assert!(!verify("wrong".into(), phc).await.unwrap());
    }

    #[tokio::test]
    async fn salts_differ() {
        let a = hash("same password".into()).await.unwrap();
        let b = hash("same password".into()).await.unwrap();
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn malformed_hash_never_verifies() {
        assert!(!verify("x".into(), "not a phc string".into()).await.unwrap());
    }

    #[tokio::test]
    async fn dummy_hash_is_a_real_hash() {
        let dummy = dummy_hash().await;
        assert!(dummy.starts_with("$argon2id$"));
    }
}
