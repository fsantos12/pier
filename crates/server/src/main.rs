//! The `pier` binary: loads configuration, opens the database, connects to Docker and
//! serves the API and web UI.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use pier_core::docker::{DockerApi, DockerClient};
use pier_server::config::Config;
use pier_server::{AppState, build_app, store};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "Usage: pier [--config <pier.toml>]\n\n\
Options:\n  -c, --config <path>  Path to the configuration file (also PIER_CONFIG)\n  -h, --help           Show this help\n  -V, --version        Show the version\n";

/// Returns the config path from `--config`, `PIER_CONFIG` or `./pier.toml`, or exits for
/// `--help` / `--version`.
fn config_path() -> Result<Option<PathBuf>> {
    let mut args = std::env::args().skip(1);
    let mut explicit = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                write!(std::io::stdout(), "{USAGE}")?;
                std::process::exit(0);
            }
            "-V" | "--version" => {
                writeln!(std::io::stdout(), "pier {}", env!("CARGO_PKG_VERSION"))?;
                std::process::exit(0);
            }
            "-c" | "--config" => {
                explicit = Some(PathBuf::from(args.next().context("--config needs a path")?));
            }
            other => anyhow::bail!("unknown argument {other:?}\n\n{USAGE}"),
        }
    }
    if explicit.is_some() {
        return Ok(explicit);
    }
    if let Some(path) = std::env::var_os("PIER_CONFIG") {
        return Ok(Some(PathBuf::from(path)));
    }
    let local = PathBuf::from("pier.toml");
    Ok(local.is_file().then_some(local))
}

async fn shutdown_signal() {
    if tokio::signal::ctrl_c().await.is_err() {
        // Without a signal handler, run until the process is killed.
        std::future::pending::<()>().await;
    }
    tracing::info!("shutting down");
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::load(config_path()?.as_deref())?;
    if config.binds_publicly() {
        tracing::warn!(
            bind = %config.bind,
            "listening on a non-loopback address without built-in HTTPS; put a TLS reverse proxy in front"
        );
    }

    let db = store::connect(&config.data_dir).await?;
    let docker =
        DockerClient::connect(&config.docker).context("invalid docker connection settings")?;
    match docker.ping().await {
        Ok(()) => tracing::info!(endpoint = %docker.endpoint(), "connected to docker"),
        Err(err) => {
            tracing::warn!(endpoint = %docker.endpoint(), error = %err, "docker is not reachable yet; requests will return 503 until it is");
        }
    }

    let bind = config.bind;
    let app = build_app(AppState::new(Arc::new(docker), db, config));
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("cannot bind {bind}"))?;
    tracing::info!(%bind, "pier listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")
}
