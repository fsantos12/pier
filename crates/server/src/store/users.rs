//! User queries.

use sqlx::{FromRow, SqlitePool};

/// A row of the `users` table.
#[derive(Debug, Clone, FromRow)]
pub struct User {
    /// Primary key.
    pub id: i64,
    /// Unique, case-insensitive account name.
    pub username: String,
    /// Argon2 PHC string.
    pub password_hash: String,
    /// Role; only `admin` exists in Phase 1.
    pub role: String,
    /// Creation time, Unix seconds.
    pub created_at: i64,
}

/// Counts users; zero means the first-run setup is still open.
///
/// # Errors
/// Propagates database errors.
pub async fn count_users(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
}

/// Inserts the first admin, but only if no user exists yet.
///
/// The check and the insert are one statement, so two concurrent setup requests can
/// never both succeed. Returns `None` when a user already existed.
///
/// # Errors
/// Propagates database errors.
pub async fn create_first_admin(
    pool: &SqlitePool,
    username: &str,
    password_hash: &str,
    now: i64,
) -> Result<Option<User>, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO users (username, password_hash, role, created_at)
         SELECT ?1, ?2, 'admin', ?3
         WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(username)
    .bind(password_hash)
    .bind(now)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Ok(None);
    }
    find_user_by_username(pool, username).await
}

/// Looks a user up by name, ignoring case.
///
/// # Errors
/// Propagates database errors.
pub async fn find_user_by_username(
    pool: &SqlitePool,
    username: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT id, username, password_hash, role, created_at FROM users WHERE username = ?1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::connect_memory;

    #[tokio::test]
    async fn only_the_first_admin_is_created() {
        let pool = connect_memory().await.unwrap();
        assert_eq!(count_users(&pool).await.unwrap(), 0);
        let first = create_first_admin(&pool, "root", "hash", 1).await.unwrap();
        assert_eq!(first.unwrap().role, "admin");
        let second = create_first_admin(&pool, "other", "hash", 2).await.unwrap();
        assert!(second.is_none());
        assert_eq!(count_users(&pool).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn username_lookup_ignores_case() {
        let pool = connect_memory().await.unwrap();
        create_first_admin(&pool, "Root", "hash", 1).await.unwrap();
        assert!(
            find_user_by_username(&pool, "rOOt")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            find_user_by_username(&pool, "nobody")
                .await
                .unwrap()
                .is_none()
        );
    }
}
