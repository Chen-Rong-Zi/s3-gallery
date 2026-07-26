//! Database query helpers — common patterns for Option<host_id> branching.

use sqlx::SqlitePool;

use crate::error::{Result, S3GalleryError};

/// Execute a query with an optional host_id binding.
///
/// If `host_id` is `Some`, the query is executed with `host_id` bound to the
/// first `?` parameter. If `None`, the `WHERE host_id = ?` clause is stripped
/// from the SQL so the query runs without host filtering.
///
/// The SQL should contain `WHERE host_id = ?` followed by either `AND ` or
/// nothing more. When `host_id` is `None`, the `host_id = ?` condition
/// is removed from the WHERE clause.
///
/// # Errors
///
/// Returns `S3GalleryError::DbError` if the query fails.
pub async fn fetch_all_opt<T>(db: &SqlitePool, sql: &str, host_id: Option<&str>) -> Result<Vec<T>>
where
    T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin,
{
    if let Some(hid) = host_id {
        sqlx::query_as::<_, T>(sql)
            .bind(hid)
            .fetch_all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))
    } else {
        let sql = strip_host_id_condition(sql);
        sqlx::query_as::<_, T>(&sql)
            .fetch_all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))
    }
}

/// Execute a scalar query with an optional host_id binding.
///
/// If `host_id` is `Some`, the query is executed with `host_id` bound to the
/// first `?` parameter. If `None`, the `WHERE host_id = ?` clause is stripped
/// from the SQL so the query runs without host filtering.
///
/// # Errors
///
/// Returns `S3GalleryError::DbError` if the query fails.
pub async fn fetch_scalar_opt<T>(db: &SqlitePool, sql: &str, host_id: Option<&str>) -> Result<T>
where
    T: for<'a> sqlx::Decode<'a, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
{
    if let Some(hid) = host_id {
        sqlx::query_scalar(sql)
            .bind(hid)
            .fetch_one(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))
    } else {
        let sql = strip_host_id_condition(sql);
        sqlx::query_scalar(&sql)
            .fetch_one(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))
    }
}

/// Strip `WHERE host_id = ?` (and the following `AND ` if present) from a SQL string.
///
/// This is used internally by `fetch_all_opt` and `fetch_scalar_opt` when
/// `host_id` is `None` to remove the host filtering condition.
fn strip_host_id_condition(sql: &str) -> String {
    if let Some(pos) = sql.find("WHERE host_id = ?") {
        let before = &sql[..pos];
        let after = &sql[pos + "WHERE host_id = ?".len()..];
        if after.starts_with(" AND ") {
            format!("{}WHERE{}", before, &after[4..])
        } else {
            format!("{}{}", before, after)
        }
    } else {
        sql.to_string()
    }
}

/// Append `AND host_id = ?` to a SQL condition when a host_id is provided.
/// Returns the modified SQL string and any bind parameters.
pub fn maybe_host_id(host_id: Option<&str>, sql: &str) -> (String, Vec<String>) {
    if let Some(hid) = host_id {
        (format!("{} AND host_id = ?", sql), vec![hid.to_string()])
    } else {
        (sql.to_string(), vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate::run_full_migration;
    use crate::db::pool::create_pool;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_fetch_all_opt_with_host_id() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let db = create_pool(&db_path).await?;
        let pool = db.get_sqlite_connection_pool().clone();
        run_full_migration(&db).await?;

        sqlx::query(
            "INSERT INTO host_config (host_id, host_name, host_type, description, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("h1").bind("Host 1").bind("test").bind("").bind("now")
        .execute(&pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query(
            "INSERT INTO host_config (host_id, host_name, host_type, description, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("h2").bind("Host 2").bind("test").bind("").bind("now")
        .execute(&pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let sql = "SELECT host_id FROM host_config WHERE host_id = ?";
        let results: Vec<(String,)> = fetch_all_opt(&pool, sql, Some("h1")).await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "h1");
        Ok(())
    }

    #[tokio::test]
    async fn test_fetch_all_opt_without_host_id() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let db = create_pool(&db_path).await?;
        let pool = db.get_sqlite_connection_pool().clone();
        run_full_migration(&db).await?;

        sqlx::query(
            "INSERT INTO host_config (host_id, host_name, host_type, description, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("h1").bind("Host 1").bind("test").bind("").bind("now")
        .execute(&pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query(
            "INSERT INTO host_config (host_id, host_name, host_type, description, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("h2").bind("Host 2").bind("test").bind("").bind("now")
        .execute(&pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let sql = "SELECT host_id FROM host_config ORDER BY host_id";
        let results: Vec<(String,)> = fetch_all_opt(&pool, sql, None).await?;
        assert_eq!(results.len(), 2);
        Ok(())
    }
}
