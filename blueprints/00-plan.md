# Pier — Project Plan

App: **Pier**. Distro: **PierOS**.

Rust app to manage Docker containers via an HTTP + WebSocket web app (gRPC later), shipped finally as a minimal Linux distro running only Docker and this program.

## Goals
- Manage networks, volumes and containers (env vars, networks, volumes, entrypoint scripts).
- Observe: 3D topology map, container logs, CPU/memory/IO metrics, real-time network traffic.
- Appliance: minimal Linux image booting straight into dockerd + this app.

## Platforms
The app is a standalone cross-platform program first; the distro is only one packaging option.
- **Windows**: connects to Docker Desktop via named pipe `//./pipe/docker_engine` (bollard supports it). Runs as a normal app or Windows service; UI opens in the browser.
- **macOS**: Docker Desktop socket (`~/.docker/run/docker.sock`).
- **Linux**: `/var/run/docker.sock`, or remote via `DOCKER_HOST` (tcp+TLS or ssh).
- Connection is configurable (local socket/pipe, TCP+TLS, SSH), so one install can also manage a remote host.
- Platform-specific features are feature-gated: eBPF traffic monitoring is Linux-only; on Windows/macOS traffic falls back to Docker stats (Docker Desktop runs containers in a Linux VM, so per-flow eBPF needs an agent inside that VM — later option).
- CI builds for x86_64-pc-windows-msvc, x86_64/aarch64 Linux (musl), and macOS.

## Architecture
```
Browser (web UI, 3D map)
   │  HTTP (REST/JSON) + WebSockets (logs, stats, events, traffic)
   ▼
app-server (axum, tokio)
   ├── docker module  (bollard → /var/run/docker.sock)
   ├── stream hub     (fan-out of logs/stats/events to WS clients)
   ├── topology       (graph of containers/networks/volumes)
   ├── traffic        (phase 4: eBPF via aya)
   └── store          (templates, scripts, settings — SQLite or files)
```

### Workspace layout (Cargo)
- `core` — domain types, Docker wrapper, topology builder (no HTTP).
- `server` — axum REST + WebSockets, auth, static UI serving.
- `traffic` — eBPF programs and loader (Linux only, feature-gated).
- `web` — frontend in Rust/WASM with Leptos; the 3D map uses three.js through JS interop (wasm-bindgen). Built into static files embedded in the binary.
- later: `grpc` — tonic service reusing `core`.

### Stream hub
One Docker stream per container (logs, stats, events) feeding a `tokio::sync::broadcast` channel; every WebSocket (and later gRPC) client subscribes, so N browsers never mean N Docker streams. Streams start on first subscriber and stop after the last leaves.

### Key crates
tokio, axum, tower-http, bollard, serde, serde_json, tracing, thiserror/anyhow, rust-embed (UI), sqlx or rusqlite, aya (eBPF), utoipa (OpenAPI docs).

## Features in detail
### Networks
Create / list / inspect / delete; driver (bridge, overlay, macvlan), subnet, gateway, labels; connect/disconnect containers.

### Volumes
Create / list / inspect / delete / prune; driver and options; show which containers use each.

### Containers
- Create from image (pull with progress over WS), start/stop/restart/remove.
- bollard mapping: `CreateNetworkOptions`, `CreateVolumeOptions`, `container::Config` (`env`, `entrypoint`, `cmd`, `exposed_ports`) and `HostConfig` (`binds`/`mounts`, `network_mode`, `port_bindings`, `restart_policy`, limits); extra networks via `connect_network` after create.
- Env vars (key/value editor, import from .env).
- Networks (attach multiple, aliases, static IP).
- Volumes / bind mounts (read-only flag).
- Entrypoint scripts: stored by the app, mounted into the container (bind or managed volume) and set as entrypoint; versioned and reusable.
- Ports, restart policy, resource limits, labels.
- Save as reusable templates.

### Observability
- Logs: live tail + history via Docker logs API, stdout/stderr, search, download.
- Metrics: CPU %, memory, block IO, net IO per container via stats stream; short in-memory history, optional persistence.
- Events: Docker events stream drives live UI updates.
- 3D topology: nodes = containers (spheres), networks (hubs), volumes (cylinders), force-directed layout; edges = attachments/mounts; colour by state; live traffic shown as animated edges.
- Real-time traffic:
  - Phase A: per-container rx/tx bytes from stats.
  - Phase B: eBPF on docker bridge/veth interfaces for per-flow (container↔container, container↔outside) traffic.

### Security
Docker socket = root access, so: login (argon2 password hashing), session/token auth, HTTPS (rustls), optional allow-list of actions, audit log.

## Phases
1. **Foundation** — Cargo workspace, bollard wrapper, REST for networks/volumes/containers, basic UI lists and forms.
2. **Containers in depth** — env vars, networks, volumes, entrypoint scripts, templates, image pull progress.
2b. **Export / import** — `.pier` bundle (zip): readable `topology.yaml` (own Compose-like schema) plus `scripts/<container>/NN-*.sh` and `dockerfiles/<name>/Dockerfile`, referenced by relative path. Import shows a preview (create vs already exists) before applying. Secret env values excluded by default, opt-in to include.
3. **Observability** — WebSocket hub, logs, metrics, events, charts.
4. **3D topology** — graph API + three.js view, live updates.
5. **Traffic monitoring** — stats-based first (all platforms), then eBPF per-flow (Linux only).
6. **gRPC API** (optional) — tonic service on top of `core`.
7. **Distro (optional packaging)** — prototype fast with Alpine diskless mode (boots into RAM from ISO/USB), then harden with LinuxKit or Buildroot (<50 MB). Init must mount proc/sys/dev/cgroup2, supervise containerd, dockerd and the app (restart on crash, e.g. runit/s6 or a small Rust init), wait for the Docker socket before starting the app, and run no extra services (no SSH/getty unless debug build). Same binary, unchanged: minimal image (LinuxKit, or Buildroot/Alpine), static musl binary, init starts containerd + dockerd + app, read-only root, persistent data partition, OTA updates later.

## Open questions
- Metrics history: in memory only, or stored (how long)?
- Windows: run as tray app, Windows service, or just a console program?
- Distro target: VM, bare-metal x86_64, ARM (Raspberry Pi)?

## Decisions
- 2026-11-18: One admin user for now; multiple users with roles later. Store users in a table with a role column from the start (Filipe).
- 2026-11-18: One Pier install manages exactly one Docker host (local or remote via configurable connection); no host ids in the API (Filipe).
- 2026-11-18: Frontend is Rust/WASM with Leptos; three.js via JS interop for the 3D map (Filipe).

- 2026-11-20: Topology export/import uses a `.pier` bundle with a Compose-like YAML plus scripts and Dockerfiles; secrets excluded by default (Filipe).
- 2026-11-20 (proposed): Docker is the source of truth for container state; SQLite stores only Pier data (templates, scripts, Dockerfiles, users, sessions, settings, audit). Containers get a `pier.template=<id>` label.
- 2026-11-20 (proposed): Phase 2 adds image search (Engine `/images/search`), tag picker (Docker Hub v2 API), and build from a user Dockerfile (`build_image`).

## Sources
Base plan by Pip; merged in from the Gemini plan: bollard type mapping, broadcast stream hub, 3D node shapes, Rust/WASM frontend option, Alpine diskless prototype.
