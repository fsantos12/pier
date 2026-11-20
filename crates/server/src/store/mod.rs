//! SQLite persistence: connection setup, migrations, users and sessions.

mod sessions;
mod users;

use std::path::Path;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

pub use sessions::{create_session, delete_session, find_session_user, purge_expired_sessions};
pub use users::{User, count_users, create_first_admin, find_user_by_username};

/// File name of the database inside the data directory.
pub const DB_FILE: &str = "pier.db";

/// Returns the current time as Unix seconds.
#[must_use]
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Opens (creating if needed) the database in `data_dir` and runs migrations.
///
/// # Errors
/// Fails when the directory cannot be created, the database cannot be opened, or a
/// migration fails.
pub async fn connect(data_dir: &Path) -> Result<SqlitePool> {
    std::fs::create_dir_all(data_dir).with_context(|| {
        format!(
            "cannot create data directory {} (set PIER_DATA_DIR or data_dir in pier.toml)",
            data_dir.display()
        )
    })?;
    let path = data_dir.join(DB_FILE);
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .with_context(|| format!("cannot open database {}", path.display()))?;
    migrate(&pool).await?;
    Ok(pool)
}

/// Opens a private in-memory database with migrations applied (for tests).
///
/// # Errors
/// Fails when SQLite or a migration fails.
pub async fn connect_memory() -> Result<SqlitePool> {
    // One connection only: every new connection to `:memory:` would be a fresh database.
    let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    migrate(&pool).await?;
    Ok(pool)
}

async fn migrate(pool: &SqlitePool) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("database migration failed")
}
