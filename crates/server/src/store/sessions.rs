//! Session queries. Rows are keyed by the SHA-256 of the session token.

use sqlx::SqlitePool;

use super::users::User;

/// Stores a session for `user_id` valid until `expires_at` (Unix seconds).
///
/// # Errors
/// Propagates database errors.
pub async fn create_session(
    pool: &SqlitePool,
    token_hash: &str,
    user_id: i64,
    now: i64,
    expires_at: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sessions (id, user_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns the owner of a live (unexpired) session.
///
/// # Errors
/// Propagates database errors.
pub async fn find_session_user(
    pool: &SqlitePool,
    token_hash: &str,
    now: i64,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT u.id, u.username, u.password_hash, u.role, u.created_at
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.id = ?1 AND s.expires_at > ?2",
    )
    .bind(token_hash)
    .bind(now)
    .fetch_optional(pool)
    .await
}

/// Deletes one session (logout).
///
/// # Errors
/// Propagates database errors.
pub async fn delete_session(pool: &SqlitePool, token_hash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE id = ?1")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Removes every expired session and returns how many were deleted.
///
/// # Errors
/// Propagates database errors.
pub async fn purge_expired_sessions(pool: &SqlitePool, now: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?1")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{connect_memory, create_first_admin};

    #[tokio::test]
    async fn sessions_expire_and_can_be_deleted() {
        let pool = connect_memory().await.unwrap();
        let user = create_first_admin(&pool, "root", "h", 0)
            .await
            .unwrap()
            .unwrap();
        create_session(&pool, "tok", user.id, 100, 200)
            .await
            .unwrap();
        assert!(
            find_session_user(&pool, "tok", 150)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            find_session_user(&pool, "tok", 200)
                .await
                .unwrap()
                .is_none(),
            "expired at the boundary"
        );
        assert!(
            find_session_user(&pool, "other", 150)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(purge_expired_sessions(&pool, 300).await.unwrap(), 1);
        create_session(&pool, "tok2", user.id, 100, 900)
            .await
            .unwrap();
        delete_session(&pool, "tok2").await.unwrap();
        assert!(
            find_session_user(&pool, "tok2", 150)
                .await
                .unwrap()
                .is_none()
        );
    }
}
