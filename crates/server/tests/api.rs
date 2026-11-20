//! In-process API tests: the real router, an in-memory database and a fake Docker host.

// Reason: integration-test crate; clippy.toml's allow-unwrap-in-tests only covers `#[test]` items.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, Response, StatusCode, header};
use http_body_util::BodyExt;
use pier_core::docker::fake::FakeDocker;
use pier_server::config::{Config, FileConfig};
use pier_server::{AppState, build_app, store};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Harness {
    app: Router,
    docker: Arc<FakeDocker>,
}

async fn harness() -> Harness {
    let docker = Arc::new(FakeDocker::new());
    let db = store::connect_memory().await.unwrap();
    let config = Config::resolve(FileConfig::default(), &|_| None).unwrap();
    let state = AppState::new(docker.clone(), db, config);
    Harness {
        app: build_app(state),
        docker,
    }
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> Response<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "localhost:8640");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    app.clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap()
}

async fn json_of(response: Response<Body>) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn session_cookie(response: &Response<Body>) -> String {
    let set = response.headers()[header::SET_COOKIE].to_str().unwrap();
    set.split(';').next().unwrap().to_string()
}

/// Runs first-run setup and returns the session cookie (`pier_session=...`).
async fn signed_in(h: &Harness) -> String {
    let response = send(
        &h.app,
        Method::POST,
        "/api/setup",
        None,
        Some(json!({"username": "admin", "password": "correct horse battery"})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    session_cookie(&response)
}

#[tokio::test]
async fn health_is_public_and_reports_docker() {
    let h = harness().await;
    let response = send(&h.app, Method::GET, "/api/health", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_of(response).await,
        json!({"status": "ok", "docker": true})
    );
    h.docker.set_unreachable(true);
    let body = json_of(send(&h.app, Method::GET, "/api/health", None, None).await).await;
    assert_eq!(body["docker"], false);
}

#[tokio::test]
async fn every_protected_route_requires_a_session() {
    let h = harness().await;
    let routes = [
        (Method::POST, "/api/auth/logout"),
        (Method::GET, "/api/auth/me"),
        (Method::GET, "/api/docs"),
        (Method::GET, "/api/system"),
        (Method::GET, "/api/networks"),
        (Method::POST, "/api/networks"),
        (Method::GET, "/api/networks/abc"),
        (Method::DELETE, "/api/networks/abc"),
        (Method::GET, "/api/volumes"),
        (Method::POST, "/api/volumes"),
        (Method::POST, "/api/volumes/prune"),
        (Method::GET, "/api/volumes/abc"),
        (Method::DELETE, "/api/volumes/abc"),
        (Method::GET, "/api/containers"),
        (Method::POST, "/api/containers"),
        (Method::GET, "/api/containers/abc"),
        (Method::DELETE, "/api/containers/abc"),
        (Method::POST, "/api/containers/abc/start"),
        (Method::POST, "/api/containers/abc/stop"),
        (Method::POST, "/api/containers/abc/restart"),
        (Method::POST, "/api/images/pull"),
    ];
    for (method, uri) in routes {
        // Unsafe methods need a JSON content type to get past the CSRF guard first.
        let body = (method != Method::GET).then(|| json!({}));
        let response = send(&h.app, method.clone(), uri, None, body.clone()).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}"
        );
        let tampered = send(
            &h.app,
            method.clone(),
            uri,
            Some("pier_session=bogus"),
            body,
        )
        .await;
        assert_eq!(
            tampered.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri} with bogus cookie"
        );
    }
}

#[tokio::test]
async fn setup_runs_once_and_logs_in() {
    let h = harness().await;
    let status = json_of(send(&h.app, Method::GET, "/api/setup/status", None, None).await).await;
    assert_eq!(status["needs_setup"], true);

    let cookie = signed_in(&h).await;
    let me = send(&h.app, Method::GET, "/api/auth/me", Some(&cookie), None).await;
    assert_eq!(me.status(), StatusCode::OK);
    assert_eq!(json_of(me).await["username"], "admin");

    let status = json_of(send(&h.app, Method::GET, "/api/setup/status", None, None).await).await;
    assert_eq!(status["needs_setup"], false);
    let again = send(
        &h.app,
        Method::POST,
        "/api/setup",
        None,
        Some(json!({"username": "second", "password": "another long password"})),
    )
    .await;
    assert_eq!(again.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn concurrent_setup_creates_exactly_one_admin() {
    let h = harness().await;
    let mut tasks = Vec::new();
    for i in 0..8 {
        let app = h.app.clone();
        tasks.push(tokio::spawn(async move {
            send(
                &app,
                Method::POST,
                "/api/setup",
                None,
                Some(json!({"username": format!("admin{i}"), "password": "correct horse battery"})),
            )
            .await
            .status()
        }));
    }
    let mut created = 0;
    for task in tasks {
        match task.await.unwrap() {
            StatusCode::CREATED => created += 1,
            StatusCode::CONFLICT => {}
            other => panic!("unexpected status {other}"),
        }
    }
    assert_eq!(created, 1);
}

#[tokio::test]
async fn setup_validates_credentials() {
    let h = harness().await;
    for body in [
        json!({"username": "", "password": "correct horse battery"}),
        json!({"username": "bad name", "password": "correct horse battery"}),
        json!({"username": "admin", "password": "short"}),
    ] {
        let response = send(&h.app, Method::POST, "/api/setup", None, Some(body)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let status = json_of(send(&h.app, Method::GET, "/api/setup/status", None, None).await).await;
    assert_eq!(
        status["needs_setup"], true,
        "failed setups must not create a user"
    );
}

#[tokio::test]
async fn login_failures_are_indistinguishable() {
    let h = harness().await;
    signed_in(&h).await;
    let wrong_password = send(
        &h.app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({"username": "admin", "password": "not the password"})),
    )
    .await;
    let unknown_user = send(
        &h.app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({"username": "ghost", "password": "not the password"})),
    )
    .await;
    assert_eq!(wrong_password.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(unknown_user.status(), StatusCode::UNAUTHORIZED);
    assert!(wrong_password.headers().get(header::SET_COOKIE).is_none());
    assert_eq!(json_of(wrong_password).await, json_of(unknown_user).await);
}

#[tokio::test]
async fn login_sets_a_hardened_cookie_and_logout_revokes_it() {
    let h = harness().await;
    signed_in(&h).await;
    let response = send(
        &h.app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({"username": "ADMIN", "password": "correct horse battery"})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let set_cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_string();
    assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Strict"));
    let cookie = session_cookie(&response);

    let ok = send(&h.app, Method::GET, "/api/system", Some(&cookie), None).await;
    assert_eq!(ok.status(), StatusCode::OK);

    let out = send(
        &h.app,
        Method::POST,
        "/api/auth/logout",
        Some(&cookie),
        Some(json!({})),
    )
    .await;
    assert_eq!(out.status(), StatusCode::NO_CONTENT);
    let after = send(&h.app, Method::GET, "/api/system", Some(&cookie), None).await;
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn repeated_login_failures_are_rate_limited() {
    let h = harness().await;
    signed_in(&h).await;
    let attempt = |password: &'static str| {
        send(
            &h.app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({"username": "admin", "password": password})),
        )
    };
    for _ in 0..5 {
        assert_eq!(
            attempt("wrong password").await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let blocked = attempt("wrong password").await;
    assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(blocked.headers().contains_key(header::RETRY_AFTER));
    // Even the right password is refused while locked out.
    assert_eq!(
        attempt("correct horse battery").await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn csrf_guard_rejects_non_json_and_cross_origin_writes() {
    let h = harness().await;
    let cookie = signed_in(&h).await;

    let form = Request::builder()
        .method(Method::POST)
        .uri("/api/volumes/prune")
        .header(header::HOST, "localhost:8640")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("a=b"))
        .unwrap();
    assert_eq!(
        h.app.clone().oneshot(form).await.unwrap().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );

    let no_type = Request::builder()
        .method(Method::POST)
        .uri("/api/volumes/prune")
        .header(header::HOST, "localhost:8640")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        h.app.clone().oneshot(no_type).await.unwrap().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );

    let cross = Request::builder()
        .method(Method::POST)
        .uri("/api/volumes/prune")
        .header(header::HOST, "localhost:8640")
        .header(header::ORIGIN, "http://evil.example")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        h.app.clone().oneshot(cross).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn names_are_validated_before_docker_is_called() {
    let h = harness().await;
    let cookie = signed_in(&h).await;
    for bad in ["a/b", "../x", "a b", "a;b", "-lead", "a$(id)"] {
        for (uri, body) in [
            ("/api/networks", json!({"name": bad})),
            ("/api/volumes", json!({"name": bad})),
            ("/api/containers", json!({"image": "alpine", "name": bad})),
        ] {
            let response = send(&h.app, Method::POST, uri, Some(&cookie), Some(body)).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri} {bad:?}");
        }
    }
    let bad_image = send(
        &h.app,
        Method::POST,
        "/api/containers",
        Some(&cookie),
        Some(json!({"image": "x;rm -rf /"})),
    )
    .await;
    assert_eq!(bad_image.status(), StatusCode::BAD_REQUEST);
    assert_eq!(h.docker.container_count(), 0);
    // Path parameters are validated as well.
    for uri in [
        "/api/containers/a%2Fb",
        "/api/networks/..%2Fx",
        "/api/volumes/a%20b",
    ] {
        let response = send(&h.app, Method::GET, uri, Some(&cookie), None).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
    }
}

#[tokio::test]
async fn dangerous_create_fields_are_rejected_not_ignored() {
    let h = harness().await;
    let cookie = signed_in(&h).await;
    let bodies = [
        json!({"image": "alpine", "privileged": true}),
        json!({"image": "alpine", "host_config": {"Binds": ["/var/run/docker.sock:/var/run/docker.sock"]}}),
        json!({"image": "alpine", "network_mode": "host"}),
        json!({"image": "alpine", "pid": "host"}),
        json!({"image": "alpine", "cap_add": ["SYS_ADMIN"]}),
        json!({"image": "alpine", "devices": ["/dev/sda"]}),
    ];
    for body in bodies {
        let response = send(
            &h.app,
            Method::POST,
            "/api/containers",
            Some(&cookie),
            Some(body),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    let opts = send(
        &h.app,
        Method::POST,
        "/api/volumes",
        Some(&cookie),
        Some(json!({"name": "v", "driver_opts": {"device": "/"}})),
    )
    .await;
    assert_eq!(opts.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(h.docker.container_count(), 0);
}

#[tokio::test]
async fn container_lifecycle_and_conflicts() {
    let h = harness().await;
    let cookie = signed_in(&h).await;
    let created = send(
        &h.app,
        Method::POST,
        "/api/containers",
        Some(&cookie),
        Some(json!({"image": "alpine:3.20", "name": "web", "cmd": ["sleep", "60"]})),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let id = json_of(created).await["container"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let dup = send(
        &h.app,
        Method::POST,
        "/api/containers",
        Some(&cookie),
        Some(json!({"image": "alpine", "name": "web"})),
    )
    .await;
    assert_eq!(dup.status(), StatusCode::CONFLICT);

    for action in ["start", "start", "stop", "stop", "restart"] {
        let uri = format!("/api/containers/{id}/{action}");
        let response = send(&h.app, Method::POST, &uri, Some(&cookie), Some(json!({}))).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT, "{action}");
    }
    // Restart leaves it running, so removal conflicts.
    let busy = send(
        &h.app,
        Method::DELETE,
        &format!("/api/containers/{id}"),
        Some(&cookie),
        Some(json!({})),
    )
    .await;
    assert_eq!(busy.status(), StatusCode::CONFLICT);
    send(
        &h.app,
        Method::POST,
        &format!("/api/containers/{id}/stop"),
        Some(&cookie),
        Some(json!({})),
    )
    .await;
    let gone = send(
        &h.app,
        Method::DELETE,
        &format!("/api/containers/{id}"),
        Some(&cookie),
        Some(json!({})),
    )
    .await;
    assert_eq!(gone.status(), StatusCode::NO_CONTENT);
    let missing = send(
        &h.app,
        Method::GET,
        &format!("/api/containers/{id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn network_and_volume_conflicts_and_prune() {
    let h = harness().await;
    let cookie = signed_in(&h).await;

    let net = send(
        &h.app,
        Method::POST,
        "/api/networks",
        Some(&cookie),
        Some(json!({"name": "backend", "subnet": "10.9.0.0/24"})),
    )
    .await;
    assert_eq!(net.status(), StatusCode::CREATED);
    let dup_net = send(
        &h.app,
        Method::POST,
        "/api/networks",
        Some(&cookie),
        Some(json!({"name": "backend"})),
    )
    .await;
    assert_eq!(
        dup_net.status(),
        StatusCode::CONFLICT,
        "duplicate network name"
    );
    h.docker.attach("backend", "web");
    let in_use = send(
        &h.app,
        Method::DELETE,
        "/api/networks/backend",
        Some(&cookie),
        Some(json!({})),
    )
    .await;
    assert_eq!(in_use.status(), StatusCode::CONFLICT);

    for name in ["keep", "drop"] {
        let response = send(
            &h.app,
            Method::POST,
            "/api/volumes",
            Some(&cookie),
            Some(json!({"name": name})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }
    let dup_vol = send(
        &h.app,
        Method::POST,
        "/api/volumes",
        Some(&cookie),
        Some(json!({"name": "keep"})),
    )
    .await;
    assert_eq!(
        dup_vol.status(),
        StatusCode::CONFLICT,
        "duplicate volume name"
    );
    h.docker.mark_volume_in_use("keep");
    let busy = send(
        &h.app,
        Method::DELETE,
        "/api/volumes/keep",
        Some(&cookie),
        Some(json!({})),
    )
    .await;
    assert_eq!(busy.status(), StatusCode::CONFLICT);
    let report = json_of(
        send(
            &h.app,
            Method::POST,
            "/api/volumes/prune",
            Some(&cookie),
            Some(json!({})),
        )
        .await,
    )
    .await;
    assert_eq!(report["volumes_deleted"], json!(["drop"]));
    let list = json_of(send(&h.app, Method::GET, "/api/volumes", Some(&cookie), None).await).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn unreachable_daemon_returns_503_everywhere() {
    let h = harness().await;
    let cookie = signed_in(&h).await;
    h.docker.set_unreachable(true);
    for uri in [
        "/api/system",
        "/api/networks",
        "/api/volumes",
        "/api/containers",
    ] {
        let response = send(&h.app, Method::GET, uri, Some(&cookie), None).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{uri}");
        let body = json_of(response).await;
        assert_eq!(body["error"], "docker daemon is unreachable");
    }
    let pull = send(
        &h.app,
        Method::POST,
        "/api/images/pull",
        Some(&cookie),
        Some(json!({"image": "alpine"})),
    )
    .await;
    assert_eq!(pull.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn image_pull_maps_missing_images_to_404() {
    let h = harness().await;
    let cookie = signed_in(&h).await;
    let ok = send(
        &h.app,
        Method::POST,
        "/api/images/pull",
        Some(&cookie),
        Some(json!({"image": "alpine:3.20"})),
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);
    let missing = send(
        &h.app,
        Method::POST,
        "/api/images/pull",
        Some(&cookie),
        Some(json!({"image": "missing/image"})),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let bad = send(
        &h.app,
        Method::POST,
        "/api/images/pull",
        Some(&cookie),
        Some(json!({"image": "a b"})),
    )
    .await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_404s_are_json_and_the_spa_serves_other_paths() {
    let h = harness().await;
    let unknown = send(&h.app, Method::GET, "/api/does-not-exist", None, None).await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_of(unknown).await["error"], "not found");

    let page = send(&h.app, Method::GET, "/containers", None, None).await;
    assert_eq!(page.status(), StatusCode::OK);
    assert!(
        page.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert_eq!(page.headers()["x-content-type-options"], "nosniff");
}

#[tokio::test]
async fn openapi_document_lists_the_routes() {
    let h = harness().await;
    let cookie = signed_in(&h).await;
    let doc = json_of(send(&h.app, Method::GET, "/api/docs", Some(&cookie), None).await).await;
    let paths = doc["paths"].as_object().unwrap();
    for path in [
        "/api/health",
        "/api/networks",
        "/api/volumes/prune",
        "/api/containers/{id}/start",
        "/api/images/pull",
    ] {
        assert!(paths.contains_key(path), "{path}");
    }
}

#[tokio::test]
async fn bad_query_strings_get_a_json_400() {
    let h = harness().await;
    let cookie = signed_in(&h).await;
    let response = send(
        &h.app,
        Method::GET,
        "/api/containers?all=maybe",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(json_of(response).await["error"].is_string());
    let ok = send(
        &h.app,
        Method::GET,
        "/api/containers?all=false",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);
}

#[tokio::test]
async fn oversized_bodies_are_refused() {
    let h = harness().await;
    let cookie = signed_in(&h).await;
    let huge = "a".repeat(200 * 1024);
    let response = send(
        &h.app,
        Method::POST,
        "/api/containers",
        Some(&cookie),
        Some(json!({"image": "alpine", "cmd": [huge]})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
