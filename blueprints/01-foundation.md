# 01 — Foundation (Phase 1)

**Branch:** `feature/foundation` (cut from `dev` by Gittie Pier)
**Status:** Approved for the Phase 1 team run (Filipe, 2026-11-20). Open questions resolved by Stormie under Filipe's full-control delegation.
**Depends on:** 00-plan.md decisions (Leptos CSR frontend, single Docker host, single admin user with role-ready schema)

## 1. Goal
A single binary that connects to one Docker host, exposes a REST API for networks, volumes and containers, protects it with a login, and serves a basic Leptos UI with lists and simple forms. No WebSockets, logs, metrics or 3D yet.

## 2. Scope
In:
- Cargo workspace with `core`, `server`, `web` crates.
- Configurable Docker connection: local socket/pipe (default), `DOCKER_HOST` (tcp+TLS, ssh).
- REST: list/inspect/create/delete networks; list/inspect/create/delete/prune volumes; list/inspect/create/start/stop/restart/remove containers (create = image, name, cmd only; env, mounts, networks come in Phase 2).
- Image pull as a blocking REST call (progress over WS comes in Phase 3).
- Login: one admin user, argon2 hash, HTTP-only session cookie. First-run setup creates the admin.
- SQLite store with `users(id, username, password_hash, role, created_at)` and `sessions`.
- Leptos UI (client-side rendered, built with Trunk) embedded into the binary with rust-embed: login page, three list pages, create forms, delete/confirm.
- `GET /api/health` and `GET /api/system` (Docker version, host OS) for the UI header.

Out (later phases): env/mount/network editors, templates, entrypoint scripts, WebSockets, logs, stats, events, topology, traffic, gRPC, HTTPS termination (see open questions).

## 3. Architecture
```
web (Leptos CSR, WASM) ──HTTP/JSON──► server (axum)
                                         ├── auth  (argon2, sessions, middleware)
                                         ├── api   (routes → core)
                                         ├── store (sqlx + SQLite, migrations)
                                         └── assets (rust-embed of web/dist)
                                       core
                                         ├── docker (bollard client + connection config)
                                         ├── model  (Network, Volume, Container DTOs, serde)
                                         └── error  (thiserror CoreError)
```
Rules:
- `core` has no HTTP or database dependency. It returns domain DTOs, never raw bollard types, so `server` and future `grpc` share one shape.
- `server` owns auth, persistence and HTTP mapping. `CoreError` maps to status codes in one place.
- `web` talks only to `/api/*`. DTOs live in a small shared crate `api-types` (serde only, compiles to WASM) so frontend and backend never drift.
- One `AppState { docker: Arc<DockerClient>, db: SqlitePool, config: Arc<Config> }` passed through axum state.

### Key crates
core: bollard, tokio, serde, thiserror, tracing.
server: axum, tower-http (trace, compression), tokio, sqlx (sqlite, migrate), argon2, rand, rust-embed, tracing-subscriber, anyhow, utoipa (OpenAPI).
web: leptos (csr), leptos_router, gloo-net, api-types. Built with Trunk.

### Config
`pier.toml` plus env overrides (`PIER_BIND`, `PIER_DATA_DIR`, `DOCKER_HOST`). Defaults: bind `127.0.0.1:8640`, data dir per OS (`%APPDATA%\Pier`, `~/Library/Application Support/Pier`, `/var/lib/pier`).

## 4. File and folder plan
```
pier/
├── Cargo.toml                  # workspace: crates/*
├── pier.example.toml
├── crates/
│   ├── core/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── error.rs
│   │       ├── docker/
│   │       │   ├── mod.rs        # DockerClient
│   │       │   ├── connection.rs # socket / pipe / tcp+tls / ssh
│   │       │   ├── networks.rs
│   │       │   ├── volumes.rs
│   │       │   ├── containers.rs
│   │       │   └── images.rs     # pull
│   │       └── model/            # domain types + From<bollard::...>
│   ├── api-types/
│   │   └── src/lib.rs            # request/response DTOs shared with web
│   ├── server/
│   │   ├── migrations/0001_init.sql
│   │   └── src/
│   │       ├── main.rs
│   │       ├── config.rs
│   │       ├── state.rs
│   │       ├── error.rs          # CoreError/AuthError → HTTP
│   │       ├── assets.rs         # rust-embed + SPA fallback
│   │       ├── auth/             # mod.rs, password.rs, session.rs, middleware.rs
│   │       ├── store/            # mod.rs, users.rs, sessions.rs
│   │       └── api/              # mod.rs, system.rs, networks.rs, volumes.rs, containers.rs, images.rs
│   └── web/
│       ├── Trunk.toml
│       ├── index.html
│       └── src/
│           ├── main.rs
│           ├── api.rs            # fetch wrapper, 401 → login redirect
│           ├── pages/            # login.rs, setup.rs, networks.rs, volumes.rs, containers.rs
│           └── components/       # table.rs, form.rs, confirm.rs, layout.rs
└── docs/blueprints/              # copies of these blueprints once the repo exists
```

## 5. REST surface (all under `/api`, JSON, auth required except where noted)
- `GET /health` (no auth), `GET /setup/status` (no auth), `POST /setup` (no auth, only while no user exists)
- `POST /auth/login`, `POST /auth/logout`, `GET /auth/me`
- `GET /system`
- `GET|POST /networks`, `GET|DELETE /networks/{id}`
- `GET|POST /volumes`, `GET|DELETE /volumes/{name}`, `POST /volumes/prune`
- `GET|POST /containers`, `GET|DELETE /containers/{id}`, `POST /containers/{id}/{start|stop|restart}`
- `POST /images/pull`
- OpenAPI at `/api/docs` (utoipa)

## 6. Implementation checklist
1. Gittie cuts `feature/foundation` from `dev`.
2. Workspace `Cargo.toml`, empty crates `core`, `api-types`, `server`, `web` (Dever can create skeleton files on request).
3. `core::docker::connection`: build bollard client from config; verify with `ping`.
4. `core::model` + `From` impls for networks, volumes, containers.
5. `core::docker` operations for networks, volumes, containers, image pull.
6. `api-types` DTOs mirroring the REST surface.
7. `server` config, state, tracing, `/health`, `/system`.
8. SQLite migration `0001_init.sql` (users with `role`, sessions) and store module.
9. Auth: setup flow, argon2 hashing, session cookie, middleware on `/api/*`.
10. REST routes for networks, volumes, containers, images; central error mapping.
11. Leptos app: router, login/setup pages, layout, three list pages, create/delete forms.
12. Embed `web/dist` with rust-embed and SPA fallback route.
13. Hunter: unit tests (mocked Docker) for every edge case below; integration tests against a real daemon.
14. Cleanie: rustfmt, clippy `-D warnings` on Windows, macOS, Linux.
15. Gittie: PR `feature/foundation` → `dev`.

## 7. Edge cases (for Hunter Pier)
Auth
- Every `/api/*` route except health and setup rejects requests without a valid session (401).
- `POST /setup` fails once any user exists, including concurrent first-run requests (only one admin is created).
- Wrong password and unknown user return the same error and similar timing.
- Expired, logged-out and tampered session cookies are rejected; cookie is HttpOnly and SameSite=Strict.
- Login brute force: repeated failures are rate-limited or delayed.
- CSRF: state-changing routes require JSON content type and SameSite cookie; cross-origin form posts fail.
Name and entrypoint injection
- Container, network and volume names are validated against Docker's allowed pattern (`[a-zA-Z0-9][a-zA-Z0-9_.-]*`); names with `/`, `..`, spaces, shell metacharacters or unicode tricks are rejected before reaching Docker.
- Image references are validated; `cmd` is passed as an argument array, never through a shell string.
- IDs in paths are validated; a name that looks like a different resource's ID prefix doesn't hit the wrong object.
Dangerous mounts and privileges
- Foundation's create endpoint accepts no mounts, `privileged`, capabilities, `network_mode: host`, `pid: host` or devices; extra fields are rejected (`deny_unknown_fields`), not silently ignored.
- Hunter keeps tests ready for Phase 2, when `docker.sock`, `/`, and privileged mounts must be blocked or require explicit confirmation.
Docker connection
- Daemon unreachable at startup and mid-request returns 503 with a clear message, no panic.
- Windows named pipe and Linux socket paths both covered; `DOCKER_HOST` tcp and ssh parse errors.
Resource operations
- Deleting a network in use, a volume in use, or a running container returns 409, not 500.
- Creating a volume or network whose name already exists returns 409 from `pier-core` for both the real client and `FakeDocker` (inspect first; Docker itself silently reuses duplicate volume names).
- Volume prune only removes unused volumes and reports what was removed.
- Starting an already running container and stopping a stopped one are no-ops or 304, not errors.
- Image pull of a nonexistent image or private registry without credentials returns a clean error; long pulls don't block other requests.
Frontend
- 401 from any call sends the user to login; the SPA fallback never serves `index.html` for `/api/*` paths.

## 8. Decisions (were open questions)
1. Bind `127.0.0.1:8640` by default; no built-in HTTPS in Phase 1. Remote access goes through a reverse proxy until a later HTTPS phase (rustls). Binding to a non-loopback address logs a warning.
2. SQLite via `sqlx` (sqlite + migrate features), so the users/roles schema can grow without a redesign.
3. Default port `8640`.
4. Root `Cargo.toml` defines `[workspace.lints]`; every crate has `[lints] workspace = true` (see style.mdc).
5. Crate package names are prefixed: `pier-core`, `pier-api-types`, `pier-server`, `pier-web` (folders stay `crates/core` etc.). A package named `core` would clash with Rust's built-in `core` crate.
6. No repo yet: work happens in `/workspace/pier` locally. The branch name `feature/foundation` applies once Filipe creates the repo.

## 9. Workflow for this run
Dever builds Phase 1 per the checklist → Hunter tests (edge cases in §7) → Cleanie polishes and comments → Hunter reruns the full suite → Filipe creates the repo → Gittie opens the Phase 1 PR when Filipe asks.
