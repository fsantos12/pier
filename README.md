# Pier

A cross-platform (Windows, macOS, Linux) Rust app that manages Docker networks, volumes and
containers from a web UI over HTTP. One binary serves the REST API and the embedded UI.
Later phases add WebSockets, logs, metrics, a 3D topology map and traffic monitoring;
the long-term plan is in `docs/blueprints/00-plan.md`, this phase in `docs/blueprints/01-foundation.md`.

## Layout

| Crate | Folder | Purpose |
| --- | --- | --- |
| `pier-core` | `crates/core` | Docker wrapper over bollard, domain types, input validation. No HTTP, no database. |
| `pier-api-types` | `crates/api-types` | Serde request/response types shared by server and web (WASM-safe). |
| `pier-server` | `crates/server` | axum REST API, argon2 login, SQLite store, embedded UI. Produces the `pier` binary. |
| `pier-web` | `crates/web` | Leptos (client-side rendered) UI, built with Trunk. |

## Prerequisites

- Rust stable (edition 2024, 1.85 or newer) with the `wasm32-unknown-unknown` target:
  `rustup target add wasm32-unknown-unknown`
- [Trunk](https://trunkrs.dev) for the web UI: `cargo install trunk --locked` (or a prebuilt binary)
- A Docker daemon to manage. Pier starts without one and answers `503` until it appears.

## Build and run

```sh
# 1. Build the web UI into crates/web/dist
cd crates/web && trunk build --release && cd ../..

# 2. Build and run the server (embeds crates/web/dist)
cargo run --release -p pier-server

# Optional: use a config file
cp pier.example.toml pier.toml
cargo run --release -p pier-server -- --config pier.toml
```

Open <http://127.0.0.1:8640>. On first start you are sent to a setup page that creates the
admin account (password: 8 to 256 characters).

### Embedding the UI

`rust-embed` bakes `crates/web/dist` into the binary at compile time. `crates/server/build.rs`
falls back to a placeholder page when `dist` does not exist, so `cargo build`, `cargo test` and
`cargo clippy` work without building the UI first. After rebuilding the UI, force the server to
re-embed it:

```sh
touch crates/server/src/assets.rs && cargo build --release -p pier-server
```

`wasm-opt` is disabled in `crates/web/index.html` (`data-wasm-opt="0"`) because the version Trunk
downloads cannot read the wasm produced by current Rust. Re-enable it once Trunk ships a newer
binaryen.

### UI development

```sh
cargo run -p pier-server            # API on 127.0.0.1:8640
cd crates/web && trunk serve        # UI on http://127.0.0.1:8080, /api proxied to Pier
```

## Configuration

Defaults, `pier.toml` keys and environment overrides are documented in `pier.example.toml`.

| Setting | Default | Environment override |
| --- | --- | --- |
| `bind` | `127.0.0.1:8640` | `PIER_BIND` |
| `data_dir` | `%APPDATA%\Pier`, `~/Library/Application Support/Pier`, `/var/lib/pier` | `PIER_DATA_DIR` |
| `docker.host` | platform default socket or named pipe | `DOCKER_HOST` |
| `docker.cert_path` | `~/.docker` | `DOCKER_CERT_PATH` |
| `docker.tls_verify` | `false` | `DOCKER_TLS_VERIFY` |

Docker connection options: `unix://` (Linux, macOS), `npipe://` (Windows), `tcp://` (plain),
`https://` or `tcp://` with `tls_verify` (TLS client certificates), `ssh://` (unix hosts only, uses the
system `ssh`). Phase 1 has no built-in HTTPS: keep the default loopback bind, or put a TLS reverse
proxy in front and set `cookie_secure = true`. Binding to a non-loopback address logs a warning.

On Linux the default data directory `/var/lib/pier` needs write access; for a quick local run set
`PIER_DATA_DIR=$HOME/.local/share/pier`.

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build -p pier-web --target wasm32-unknown-unknown   # or: trunk build
```

Unit tests use `pier_core::docker::fake::FakeDocker` (feature `test-util`), an in-memory Docker
host, so no daemon is needed. Integration tests against a real daemon come with Hunter's suite.

## API

REST under `/api`, JSON only. Public: `GET /api/health`, `GET /api/setup/status`, `POST /api/setup`,
`POST /api/auth/login`. Everything else needs the `pier_session` cookie (`HttpOnly`,
`SameSite=Strict`) and, for state-changing requests, `Content-Type: application/json`.
The `OpenAPI` document is served (after login) at `GET /api/docs`.
