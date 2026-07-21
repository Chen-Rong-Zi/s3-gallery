use sqlx::SqlitePool;

use crate::error::OssgalleyError;
use crate::error::Result;

/// Execute a SQL query on the pool and convert `sqlx::Error` to
/// `OssgalleyError::DbError`.
async fn execute_query(pool: &SqlitePool, sql: &str) -> Result<()> {
    sqlx::query(sql)
        .execute(pool)
        .await
        .map_err(|e| OssgalleyError::DbError(format!("SQL error: {e}")))?;
    Ok(())
}

/// Run all schema migrations.
///
/// Enables WAL mode and foreign keys, then creates all 9 tables and 7 indexes
/// if they do not already exist.  The DDL statements are idempotent, so running
/// this function multiple times is safe.
///
/// # Errors
///
/// Returns `OssgalleyError::DbError` if any SQL statement fails.
/// Returns `OssgalleyError::MigrationError` if the schema version cannot be
/// set after migration.
pub async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    // Enable WAL mode and foreign keys (PRAGMAs must be run outside a
    // transaction).
    execute_query(pool, "PRAGMA journal_mode = WAL;").await?;
    execute_query(pool, "PRAGMA foreign_keys = ON;").await?;

    // -- Tables -----------------------------------------------------------

    // host_config
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS host_config (
            host_id TEXT PRIMARY KEY,
            host_name TEXT NOT NULL,
            host_type TEXT NOT NULL DEFAULT 'unknown',
            description TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );",
    )
    .await?;

    // files
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS files (
            key TEXT PRIMARY KEY,
            etag TEXT NOT NULL,
            size INTEGER NOT NULL,
            last_modified TEXT NOT NULL,
            content_type TEXT,
            file_type TEXT NOT NULL,
            metadata_state TEXT NOT NULL DEFAULT 'pending',
            is_deleted INTEGER NOT NULL DEFAULT 0
        );",
    )
    .await?;

    // classification_rules
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS classification_rules (
            extension TEXT PRIMARY KEY,
            file_type TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            description TEXT
        );",
    )
    .await?;

    // extractor_rules
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS extractor_rules (
            extension TEXT NOT NULL,
            extractor_name TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (extension, extractor_name)
        );",
    )
    .await?;

    // metadata
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS metadata (
            file_key TEXT NOT NULL,
            namespace TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            extracted_at TEXT NOT NULL,
            partial INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (file_key, namespace, key),
            FOREIGN KEY (file_key) REFERENCES files(key) ON DELETE CASCADE
        );",
    )
    .await?;

    // thumbnails
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS thumbnails (
            file_key TEXT PRIMARY KEY,
            data BLOB NOT NULL,
            format TEXT NOT NULL DEFAULT 'jpeg',
            width INTEGER,
            height INTEGER,
            cached_at TEXT NOT NULL,
            FOREIGN KEY (file_key) REFERENCES files(key) ON DELETE CASCADE
        );",
    )
    .await?;

    // tags
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS tags (
            tag_id INTEGER PRIMARY KEY AUTOINCREMENT,
            tag_name TEXT NOT NULL UNIQUE,
            tag_type TEXT NOT NULL DEFAULT 'auto'
        );",
    )
    .await?;

    // file_tags
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS file_tags (
            file_key TEXT NOT NULL,
            tag_id INTEGER NOT NULL,
            PRIMARY KEY (file_key, tag_id),
            FOREIGN KEY (file_key) REFERENCES files(key) ON DELETE CASCADE,
            FOREIGN KEY (tag_id) REFERENCES tags(tag_id) ON DELETE CASCADE
        );",
    )
    .await?;

    // scan_metadata
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS scan_metadata (
            last_scanned_key TEXT,
            last_scanned_at TEXT,
            total_files INTEGER,
            total_size INTEGER,
            db_schema_version INTEGER NOT NULL DEFAULT 1
        );",
    )
    .await?;

    // -- Indexes ----------------------------------------------------------

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_files_file_type ON files(file_type);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_files_last_modified ON files(last_modified);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_metadata_namespace ON metadata(namespace);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_metadata_file_key ON metadata(file_key);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_metadata_key_value ON metadata(key, value);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_tags_tag_type ON tags(tag_type);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_thumbnails_cached_at ON thumbnails(cached_at);",
    )
    .await?;

    // -- Schema version ---------------------------------------------------

    // Set the schema version to 1.  Use INSERT OR IGNORE so that re-running
    // the migration does not fail if a row already exists.
    sqlx::query(
        "INSERT OR IGNORE INTO scan_metadata (db_schema_version) VALUES (1);",
    )
    .execute(pool)
    .await
    .map_err(|e| {
        OssgalleyError::MigrationError(format!("Failed to set schema version: {e}"))
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::error::OssgalleyError;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_run_migrations_creates_tables() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        // Verify all 9 user tables exist (sqlite_sequence is auto-generated
        // for AUTOINCREMENT columns and is excluded from the count).
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| OssgalleyError::DbError(e.to_string()))?;

        let expected_tables = [
            "classification_rules",
            "extractor_rules",
            "file_tags",
            "files",
            "host_config",
            "metadata",
            "scan_metadata",
            "tags",
            "thumbnails",
        ];

        for name in &expected_tables {
            assert!(
                tables.contains(&name.to_string()),
                "table {name} should exist"
            );
        }

        assert_eq!(tables.len(), expected_tables.len());

        dir.close().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        Ok(())
    }

    #[tokio::test]
    async fn test_migration_is_idempotent() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;

        // Run migrations twice
        run_migrations(&pool).await?;
        run_migrations(&pool).await?;

        // Verify tables still exist
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| OssgalleyError::DbError(e.to_string()))?;

        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"metadata".to_string()));

        dir.close().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        Ok(())
    }

    #[tokio::test]
    async fn test_all_indexes_created() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let indexes: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='index' AND name IS NOT NULL ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| OssgalleyError::DbError(e.to_string()))?;

        let expected_indexes = [
            "idx_files_file_type",
            "idx_files_last_modified",
            "idx_metadata_file_key",
            "idx_metadata_key_value",
            "idx_metadata_namespace",
            "idx_tags_tag_type",
            "idx_thumbnails_cached_at",
        ];

        for name in &expected_indexes {
            assert!(
                indexes.contains(&name.to_string()),
                "index {name} should exist"
            );
        }

        dir.close().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        Ok(())
    }

    #[tokio::test]
    async fn test_schema_version_set() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let version: i64 = sqlx::query_scalar(
            "SELECT db_schema_version FROM scan_metadata LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .map_err(|e| OssgalleyError::DbError(e.to_string()))?;

        assert_eq!(version, 1);

        dir.close().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        Ok(())
    }

    #[tokio::test]
    async fn test_foreign_keys_enabled() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        // PRAGMA foreign_keys returns 0 or 1.
        let fk_enabled: i32 = sqlx::query_scalar("PRAGMA foreign_keys;")
            .fetch_one(&pool)
            .await
            .map_err(|e| OssgalleyError::DbError(e.to_string()))?;

        assert_eq!(fk_enabled, 1, "foreign keys should be enabled");

        dir.close().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        Ok(())
    }
}