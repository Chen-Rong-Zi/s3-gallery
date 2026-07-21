use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

use crate::error::OssgalleyError;
use crate::error::Result;

/// Create a new SQLite connection pool.
///
/// The database file will be created if it does not exist. WAL journal mode and
/// foreign key enforcement are enabled at the connection level.
///
/// # Errors
///
/// Returns `OssgalleyError::DbError` if the pool cannot be created
/// (e.g. invalid path, permissions, or sqlite incompatibility).
pub async fn create_pool(db_path: &Path) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal);

    SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .map_err(|e| OssgalleyError::DbError(format!("Failed to create pool: {e}")))
}