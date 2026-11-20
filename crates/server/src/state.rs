//! Shared application state handed to every axum handler.

use std::sync::Arc;

use pier_core::docker::DockerApi;
use sqlx::SqlitePool;

use crate::auth::LoginLimiter;
use crate::config::Config;

/// State cloned into each request. Everything inside is cheap to clone (`Arc` or a pool).
#[derive(Clone)]
pub struct AppState {
    /// The Docker host. A trait object so tests can swap in a fake.
    pub docker: Arc<dyn DockerApi>,
    /// SQLite connection pool.
    pub db: SqlitePool,
    /// Immutable runtime configuration.
    pub config: Arc<Config>,
    /// Failed-login tracker used for brute-force protection.
    pub limiter: Arc<LoginLimiter>,
}

impl AppState {
    /// Assembles the state from its parts.
    #[must_use]
    pub fn new(docker: Arc<dyn DockerApi>, db: SqlitePool, config: Config) -> Self {
        Self {
            docker,
            db,
            config: Arc::new(config),
            limiter: Arc::new(LoginLimiter::default()),
        }
    }
}
