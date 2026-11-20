//! Integration tests for `DockerClient` against a real Docker daemon.
//!
//! Every test is `#[ignore = "needs docker"]`; run them with
//! `cargo test -p pier-core --test docker_real -- --ignored --test-threads=4`.
//! Each test names its resources `pier-test-<unique>-...` and a drop guard removes
//! everything with that prefix, even when the test panics.
//!
//! The prune test also removes *other* unused volumes on the daemon, so it only runs
//! when `PIER_TEST_ALLOW_PRUNE=1` is set (CI and throwaway machines only).

// Reason: integration-test crate; clippy.toml's allow-unwrap-in-tests only covers `#[test]` items.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use pier_core::CoreError;
use pier_core::docker::{ConnectionConfig, DockerApi, DockerClient};
use pier_core::model::{ContainerCreate, NetworkCreate, VolumeCreate};

const IMAGE: &str = "alpine:3.20";

/// Removes every container, network and volume whose name starts with `prefix`.
struct Cleanup {
    prefix: String,
}

impl Cleanup {
    fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self {
            prefix: format!("pier-test-{}-{nanos:x}-{n}", std::process::id()),
        }
    }

    fn name(&self, suffix: &str) -> String {
        format!("{}-{suffix}", self.prefix)
    }
}

fn docker_cli(args: &[&str]) -> String {
    let out = Command::new("docker")
        .args(args)
        .output()
        .expect("docker CLI");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let filter = format!("name={}", self.prefix);
        for id in docker_cli(&["ps", "-aq", "--filter", &filter]).lines() {
            docker_cli(&["rm", "-f", "-v", id]);
        }
        for id in docker_cli(&["network", "ls", "-q", "--filter", &filter]).lines() {
            docker_cli(&["network", "rm", id]);
        }
        for name in docker_cli(&["volume", "ls", "-q", "--filter", &filter]).lines() {
            docker_cli(&["volume", "rm", "-f", name]);
        }
    }
}

fn client() -> DockerClient {
    DockerClient::connect(&ConnectionConfig::default()).expect("connect")
}

async fn ensure_image(c: &DockerClient) {
    c.pull_image(IMAGE).await.expect("pull test image");
}

fn sleeper(name: &str) -> ContainerCreate {
    ContainerCreate {
        image: IMAGE.into(),
        name: Some(name.into()),
        cmd: vec!["sleep".into(), "300".into()],
    }
}

#[tokio::test]
#[ignore = "needs docker"]
async fn ping_and_system_info() {
    let c = client();
    c.ping().await.unwrap();
    let info = c.system_info().await.unwrap();
    assert_ne!(info.docker_version, "");
}

#[tokio::test]
#[ignore = "needs docker"]
async fn network_crud_and_duplicate_name_is_conflict() {
    let g = Cleanup::new();
    let c = client();
    let name = g.name("net");
    let spec = NetworkCreate {
        name: name.clone(),
        driver: Some("bridge".into()),
        internal: true,
        subnet: None,
        gateway: None,
    };
    let created = c.create_network(spec.clone()).await.unwrap();
    assert_eq!(created.name, name);
    assert!(created.internal);
    assert!(
        c.list_networks()
            .await
            .unwrap()
            .iter()
            .any(|n| n.name == name)
    );
    assert!(matches!(
        c.create_network(spec).await,
        Err(CoreError::Conflict(_))
    ));
    c.remove_network(&name).await.unwrap();
    assert!(matches!(
        c.inspect_network(&name).await,
        Err(CoreError::NotFound(_))
    ));
}

/// Blueprint §7: Docker silently reuses duplicate volume names, so `pier-core` must
/// inspect first and return `Conflict` for the real client as well as `FakeDocker`.
#[tokio::test]
#[ignore = "needs docker"]
async fn volume_duplicate_name_is_conflict() {
    let g = Cleanup::new();
    let c = client();
    let spec = VolumeCreate {
        name: g.name("vol"),
        driver: None,
    };
    c.create_volume(spec.clone()).await.unwrap();
    assert!(matches!(
        c.create_volume(spec).await,
        Err(CoreError::Conflict(_))
    ));
}

#[tokio::test]
#[ignore = "needs docker"]
async fn deleting_in_use_network_and_volume_is_conflict() {
    let g = Cleanup::new();
    let c = client();
    ensure_image(&c).await;
    let net = g.name("net");
    let vol = g.name("vol");
    c.create_network(NetworkCreate {
        name: net.clone(),
        driver: None,
        internal: false,
        subnet: None,
        gateway: None,
    })
    .await
    .unwrap();
    c.create_volume(VolumeCreate {
        name: vol.clone(),
        driver: None,
    })
    .await
    .unwrap();
    // Phase 1 cannot attach mounts or networks, so the user is set up with the CLI.
    let mount = format!("{vol}:/data");
    docker_cli(&[
        "run",
        "-d",
        "--name",
        &g.name("user"),
        "--network",
        &net,
        "-v",
        &mount,
        IMAGE,
        "sleep",
        "300",
    ]);
    assert!(matches!(
        c.remove_network(&net).await,
        Err(CoreError::Conflict(_) | CoreError::Forbidden(_))
    ));
    assert!(matches!(
        c.remove_volume(&vol).await,
        Err(CoreError::Conflict(_))
    ));
}

#[tokio::test]
#[ignore = "needs docker"]
async fn container_lifecycle_is_idempotent_and_conflicts_are_reported() {
    let g = Cleanup::new();
    let c = client();
    ensure_image(&c).await;
    let name = g.name("ctr");
    let detail = c.create_container(sleeper(&name)).await.unwrap();
    assert_eq!(detail.cmd, vec!["sleep", "300"]);
    assert_eq!(detail.mounts.len(), 0);

    c.start_container(&name).await.unwrap();
    // Starting a running container is a no-op (Docker answers 304).
    c.start_container(&name).await.unwrap();
    assert!(
        c.inspect_container(&name)
            .await
            .unwrap()
            .state_detail
            .running
    );

    assert!(matches!(
        c.remove_container(&name).await,
        Err(CoreError::Conflict(_))
    ));

    c.restart_container(&name).await.unwrap();
    c.stop_container(&name).await.unwrap();
    // Stopping a stopped container is a no-op too.
    c.stop_container(&name).await.unwrap();
    assert!(
        !c.inspect_container(&name)
            .await
            .unwrap()
            .state_detail
            .running
    );

    c.remove_container(&name).await.unwrap();
    assert!(matches!(
        c.inspect_container(&name).await,
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        c.start_container(&name).await,
        Err(CoreError::NotFound(_))
    ));
}

/// `cmd` goes to Docker as an argument array, so shell syntax stays literal.
#[tokio::test]
#[ignore = "needs docker"]
async fn command_arguments_are_not_shell_interpreted() {
    let g = Cleanup::new();
    let c = client();
    ensure_image(&c).await;
    let name = g.name("echo");
    let payload = "$(id) `id`; touch /tmp/pwned";
    c.create_container(ContainerCreate {
        image: IMAGE.into(),
        name: Some(name.clone()),
        cmd: vec!["echo".into(), payload.into()],
    })
    .await
    .unwrap();
    c.start_container(&name).await.unwrap();
    docker_cli(&["wait", &name]);
    assert_eq!(docker_cli(&["logs", &name]), payload);
}

#[tokio::test]
#[ignore = "needs docker"]
async fn existing_name_wins_over_another_containers_id_prefix() {
    let g = Cleanup::new();
    let c = client();
    ensure_image(&c).await;
    let other = c.create_container(sleeper(&g.name("other"))).await.unwrap();
    // A short hex name equal to the other container's ID prefix (allowed: < 12 chars).
    let prefix = other.container.id[..6].to_string();
    let named = c
        .create_container(ContainerCreate {
            image: IMAGE.into(),
            name: Some(prefix.clone()),
            cmd: vec!["true".into()],
        })
        .await
        .unwrap();
    let guard_name = prefix.clone();
    let resolved = c.inspect_container(&prefix).await.unwrap();
    docker_cli(&["rm", "-f", &guard_name]);
    assert_eq!(resolved.container.id, named.container.id);
}

#[tokio::test]
#[ignore = "needs docker"]
async fn bad_identifiers_never_reach_the_daemon() {
    let c = client();
    for bad in ["../../info", "a/b", "x?force=1", "..", "%2e%2e", "a#b"] {
        assert!(
            matches!(
                c.inspect_container(bad).await,
                Err(CoreError::InvalidInput(_))
            ),
            "{bad}"
        );
        assert!(
            matches!(c.remove_volume(bad).await, Err(CoreError::InvalidInput(_))),
            "{bad}"
        );
        assert!(
            matches!(c.remove_network(bad).await, Err(CoreError::InvalidInput(_))),
            "{bad}"
        );
    }
}

#[tokio::test]
#[ignore = "needs docker"]
async fn pulling_a_missing_image_is_not_found() {
    let c = client();
    let result = c
        .pull_image("pier-test-does-not-exist/nope:missing-tag")
        .await;
    assert!(matches!(result, Err(CoreError::NotFound(_))), "{result:?}");
}

/// Blueprint §7: prune removes only unused volumes and reports what it removed.
#[tokio::test]
#[ignore = "needs docker"]
async fn prune_removes_unused_volumes_only() {
    if std::env::var("PIER_TEST_ALLOW_PRUNE").as_deref() != Ok("1") {
        eprintln!("skipped: set PIER_TEST_ALLOW_PRUNE=1 to allow pruning this daemon");
        return;
    }
    let g = Cleanup::new();
    let c = client();
    ensure_image(&c).await;
    let unused = g.name("unused");
    let used = g.name("used");
    for name in [&unused, &used] {
        c.create_volume(VolumeCreate {
            name: name.clone(),
            driver: None,
        })
        .await
        .unwrap();
    }
    let mount = format!("{used}:/data");
    docker_cli(&[
        "run",
        "-d",
        "--name",
        &g.name("user"),
        "-v",
        &mount,
        IMAGE,
        "sleep",
        "300",
    ]);

    let report = c.prune_volumes().await.unwrap();
    assert!(!report.volumes_deleted.contains(&used));
    c.inspect_volume(&used).await.unwrap();
    assert!(
        report.volumes_deleted.contains(&unused),
        "unused named volume was not pruned: {report:?}"
    );
}

/// Regression: parallel creates of one volume name must yield exactly one success
/// (`pier-core` serialises check-then-create per name).
#[tokio::test]
#[ignore = "needs docker"]
async fn parallel_volume_creates_yield_one_success() {
    let g = Cleanup::new();
    let c = std::sync::Arc::new(client());
    let name = g.name("race");
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let (c, name) = (c.clone(), name.clone());
        tasks.push(tokio::spawn(async move {
            c.create_volume(VolumeCreate { name, driver: None })
                .await
                .is_ok()
        }));
    }
    let mut ok = 0;
    for t in tasks {
        ok += usize::from(t.await.unwrap());
    }
    assert_eq!(ok, 1, "{ok} parallel creates succeeded");
}
