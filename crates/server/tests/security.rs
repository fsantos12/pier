//! Security edge cases for the HTTP API (blueprint 01 §7) not covered by `api.rs`:
//! session expiry and tampering, token storage, rate-limiter robustness, CSRF origins,
//! encoded path tricks, secret leakage and Host-header (DNS rebinding) checks.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, Response, StatusCode, header};
use http_body_util::BodyExt;
use pier_core::docker::fake::FakeDocker;
use pier_server::auth::LoginLimiter;
use pier_server::config::{Config, FileConfig};
use pier_server::{AppState, build_app, store};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tower::ServiceExt;

const PASSWORD: &str = "correct horse battery";

struct Harness {
    app: Router,
    db: SqlitePool,
}

async fn harness() -> Harness {
    let db = store::connect_memory().await.unwrap();
    let config = Config::resolve(FileConfig::default(), &|_| None).unwrap();
    let state = AppState::new(Arc::new(FakeDocker::new()), db.clone(), config);
    Harness {
        app: build_app(state),
        db,
    }
}

fn request(method: Method, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "localhost:8640")
}

async fn call(app: &Router, req: Request<Body>) -> Response<Body> {
    app.clone().oneshot(req).await.unwrap()
}

async fn body_text(response: Response<Body>) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn json_req(method: Method, uri: &str, cookie: Option<&str>, body: &Value) -> Request<Body> {
    let mut b = request(method, uri).header(header::CONTENT_TYPE, "application/json");
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    b.body(Body::from(body.to_string())).unwrap()
}

fn get(uri: &str, cookie: &str) -> Request<Body> {
    request(Method::GET, uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
}

/// Runs setup and returns `(cookie header value, raw token)`.
async fn signed_in(h: &Harness) -> (String, String) {
    let res = call(
        &h.app,
        json_req(
            Method::POST,
            "/api/setup",
            None,
            &json!({"username": "admin", "password": PASSWORD}),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let set = res.headers()[header::SET_COOKIE].to_str().unwrap();
    let pair = set.split(';').next().unwrap().to_string();
    let token = pair.split_once('=').unwrap().1.to_string();
    (pair, token)
}

#[tokio::test]
async fn expired_sessions_are_rejected() {
    let h = harness().await;
    let (cookie, _) = signed_in(&h).await;
    assert_eq!(
        call(&h.app, get("/api/auth/me", &cookie)).await.status(),
        StatusCode::OK
    );
    sqlx::query("UPDATE sessions SET expires_at = 0")
        .execute(&h.db)
        .await
        .unwrap();
    assert_eq!(
        call(&h.app, get("/api/auth/me", &cookie)).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn tampered_and_malformed_cookies_are_rejected() {
    let h = harness().await;
    let (_, token) = signed_in(&h).await;
    let mut flipped = token.clone().into_bytes();
    flipped[0] = if flipped[0] == b'A' { b'B' } else { b'A' };
    let flipped = String::from_utf8(flipped).unwrap();
    for cookie in [
        format!("pier_session={flipped}"),
        format!("pier_session={token}x"),
        format!("pier_session={}", &token[..token.len() - 1]),
        "pier_session=".to_string(),
        format!("pier_session={}", "A".repeat(4096)),
        format!("PIER_SESSION={token}"),
        format!("xpier_session={token}"),
        "pier_session=' OR '1'='1".to_string(),
    ] {
        assert_eq!(
            call(&h.app, get("/api/auth/me", &cookie)).await.status(),
            StatusCode::UNAUTHORIZED,
            "{cookie}"
        );
    }
    // The real token still works among other cookies.
    let mixed = format!("theme=dark; pier_session={token}; other=1");
    assert_eq!(
        call(&h.app, get("/api/auth/me", &mixed)).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn session_tokens_are_stored_hashed() {
    let h = harness().await;
    let (_, token) = signed_in(&h).await;
    let ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM sessions")
        .fetch_all(&h.db)
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);
    assert_ne!(ids[0].0, token, "raw session token must not be stored");
    assert!(!ids[0].0.contains(&token));
}

#[tokio::test]
async fn password_is_stored_as_argon2_and_never_echoed() {
    let h = harness().await;
    signed_in(&h).await;
    let (hash,): (String,) = sqlx::query_as("SELECT password_hash FROM users")
        .fetch_one(&h.db)
        .await
        .unwrap();
    assert!(hash.starts_with("$argon2"), "{hash}");
    assert!(!hash.contains(PASSWORD));

    let wrong = "hunter2-wrong-password";
    let res = call(
        &h.app,
        json_req(
            Method::POST,
            "/api/auth/login",
            None,
            &json!({"username": "admin", "password": wrong}),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert!(!body_text(res).await.contains(wrong));

    let res = call(
        &h.app,
        json_req(
            Method::POST,
            "/api/setup",
            None,
            &json!({"username": "x", "password": PASSWORD}),
        ),
    )
    .await;
    assert!(!body_text(res).await.contains(PASSWORD));
}

#[tokio::test]
async fn each_login_issues_a_fresh_token() {
    let h = harness().await;
    let (_, first) = signed_in(&h).await;
    let res = call(
        &h.app,
        json_req(
            Method::POST,
            "/api/auth/login",
            None,
            &json!({"username": "admin", "password": PASSWORD}),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let set = res.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_string();
    assert!(!set.contains(&first));
}

#[tokio::test]
async fn cross_origin_writes_are_refused_for_every_origin_shape() {
    let h = harness().await;
    let (cookie, _) = signed_in(&h).await;
    for origin in [
        "http://evil.example",
        "null",
        "http://localhost:8641",
        "http://localhost:8640.evil.example",
        "http://evil.example/localhost:8640",
        "",
    ] {
        let req = request(Method::POST, "/api/volumes")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::COOKIE, &cookie)
            .header(header::ORIGIN, origin)
            .body(Body::from(json!({"name": "v"}).to_string()))
            .unwrap();
        assert_eq!(
            call(&h.app, req).await.status(),
            StatusCode::FORBIDDEN,
            "{origin:?}"
        );
    }
}

#[tokio::test]
async fn encoded_path_tricks_get_400_or_404_never_500() {
    let h = harness().await;
    let (cookie, _) = signed_in(&h).await;
    for uri in [
        "/api/containers/..%2F..%2Finfo",
        "/api/containers/%2e%2e",
        "/api/containers/a%3Fforce%3D1",
        "/api/containers/a%23b",
        "/api/containers/%00",
        "/api/volumes/..%5c..%5cetc",
        "/api/networks/%E2%80%AEevil",
        "/api/containers/a%20b/start",
    ] {
        let status = call(&h.app, get(uri, &cookie)).await.status();
        assert!(
            matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ),
            "{uri} -> {status}"
        );
    }
}

#[tokio::test]
async fn lockout_is_case_insensitive() {
    let limiter = LoginLimiter::default();
    let now = Instant::now();
    for _ in 0..5 {
        limiter.record_failure("admin", now);
    }
    assert!(limiter.check("ADMIN", now).is_err());
    assert!(limiter.check("Admin", now).is_err());
}

/// An attacker who fails logins for many throwaway usernames must not be able to evict
/// the victim's lockout entry and reset its brute-force counter.
#[tokio::test]
async fn lockout_survives_a_flood_of_other_usernames() {
    let limiter = LoginLimiter::default();
    let now = Instant::now();
    for _ in 0..5 {
        limiter.record_failure("admin", now);
    }
    assert!(limiter.check("admin", now).is_err());
    // Junk failures arrive after the victim's, as they would in a real attack.
    let later = now + std::time::Duration::from_millis(1);
    for i in 0..10_050 {
        limiter.record_failure(&format!("junk{i}"), later);
    }
    assert!(
        limiter.check("admin", later).is_err(),
        "admin's lockout was evicted by unrelated usernames"
    );
}

/// DNS rebinding: a page on evil.example rebinds its name to 127.0.0.1 and posts to
/// /api/setup with Host and Origin both `evil.example:8640`, so the Origin check passes.
/// On a fresh install it could claim the admin account. Pier should only accept Host
/// values it serves (loopback names or a configured public name).
#[tokio::test]
#[ignore = "known issue: no Host allow-list; DNS rebinding can claim first-run setup"]
async fn foreign_host_header_cannot_claim_setup() {
    let h = harness().await;
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/setup")
        .header(header::HOST, "evil.example:8640")
        .header(header::ORIGIN, "http://evil.example:8640")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"username": "attacker", "password": PASSWORD}).to_string(),
        ))
        .unwrap();
    let status = call(&h.app, req).await.status();
    assert!(
        matches!(
            status,
            StatusCode::FORBIDDEN | StatusCode::MISDIRECTED_REQUEST | StatusCode::BAD_REQUEST
        ),
        "setup accepted from a foreign Host: {status}"
    );
}

#[tokio::test]
async fn concurrent_logins_with_valid_and_invalid_passwords_stay_consistent() {
    let h = harness().await;
    signed_in(&h).await;
    let mut tasks = Vec::new();
    for i in 0..20 {
        let app = h.app.clone();
        let password = if i % 2 == 0 {
            PASSWORD
        } else {
            "wrong password"
        };
        tasks.push(tokio::spawn(async move {
            let res = call(
                &app,
                json_req(
                    Method::POST,
                    "/api/auth/login",
                    None,
                    &json!({"username": "admin", "password": password}),
                ),
            )
            .await;
            (i % 2 == 0, res.status())
        }));
    }
    for t in tasks {
        let (good, status) = t.await.unwrap();
        if good {
            assert!(
                matches!(status, StatusCode::OK | StatusCode::TOO_MANY_REQUESTS),
                "{status}"
            );
        } else {
            assert!(
                matches!(
                    status,
                    StatusCode::UNAUTHORIZED | StatusCode::TOO_MANY_REQUESTS
                ),
                "{status}"
            );
        }
    }
}

/// With the map full of locked junk entries, the admin must still be rate-limited.
#[tokio::test]
async fn admin_stays_limited_when_the_map_is_full_of_locked_junk() {
    let limiter = LoginLimiter::default();
    let now = Instant::now();
    for i in 0..10_050 {
        for _ in 0..5 {
            limiter.record_failure(&format!("junk{i}"), now);
        }
    }
    let later = now + std::time::Duration::from_millis(1);
    let mut locked = false;
    for _ in 0..20 {
        if limiter.check("admin", later).is_err() {
            locked = true;
            break;
        }
        limiter.record_failure("admin", later);
    }
    assert!(locked, "admin was never rate-limited with a full map");
}
