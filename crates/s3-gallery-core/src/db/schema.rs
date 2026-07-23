use sqlx::SqlitePool;

use crate::error::Result;
use crate::error::S3GalleryError;

/// Execute a SQL query on the pool and convert `sqlx::Error` to
/// `S3GalleryError::DbError`.
async fn execute_query(pool: &SqlitePool, sql: &str) -> Result<()> {
    sqlx::query(sql)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("SQL error: {e}")))?;
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
/// Returns `S3GalleryError::DbError` if any SQL statement fails.
/// Returns `S3GalleryError::MigrationError` if the schema version cannot be
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

    // Attempt to add bucket column to host_config (ignore if already exists)
    drop(
        sqlx::query("ALTER TABLE host_config ADD COLUMN bucket TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await,
    );

    // Attempt to add endpoint and region columns to host_config (ignore if already exist)
    drop(
        sqlx::query("ALTER TABLE host_config ADD COLUMN endpoint TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await,
    );
    drop(
        sqlx::query("ALTER TABLE host_config ADD COLUMN region TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await,
    );

    // files
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS files (
            host_id TEXT NOT NULL,
            key TEXT NOT NULL,
            etag TEXT NOT NULL,
            size INTEGER NOT NULL,
            last_modified TEXT NOT NULL,
            content_type TEXT,
            file_type TEXT NOT NULL,
            metadata_state TEXT NOT NULL DEFAULT 'pending',
            is_deleted INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (host_id, key)
        );",
    )
    .await?;

    // Attempt to add effective_date column to files (ignore if already exists)
    drop(
        sqlx::query("ALTER TABLE files ADD COLUMN effective_date TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await,
    );

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
            PRIMARY KEY (file_key, namespace, key)
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
            cached_at TEXT NOT NULL
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
            FOREIGN KEY (tag_id) REFERENCES tags(tag_id) ON DELETE CASCADE
        );",
    )
    .await?;

    // scan_metadata
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS scan_metadata (
            host_id TEXT NOT NULL PRIMARY KEY,
            last_scanned_key TEXT,
            last_scanned_at TEXT,
            total_files INTEGER,
            total_size INTEGER,
            db_schema_version INTEGER NOT NULL DEFAULT 1
        );",
    )
    .await?;

    // dir_sizes
    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS dir_sizes (
            host_id TEXT NOT NULL,
            dir_path TEXT NOT NULL,
            total_size INTEGER NOT NULL DEFAULT 0,
            total_files INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (host_id, dir_path)
        );",
    )
    .await?;

    // -- Traffic tracking tables --------------------------------------------

    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS traffic_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            host_id TEXT NOT NULL,
            operation TEXT NOT NULL,
            business TEXT NOT NULL,
            direction TEXT NOT NULL,
            bytes INTEGER NOT NULL,
            count INTEGER NOT NULL,
            recorded_at TEXT NOT NULL
        );",
    )
    .await?;

    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS traffic_file_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            host_id TEXT NOT NULL,
            file_key TEXT NOT NULL,
            business TEXT NOT NULL,
            bytes INTEGER NOT NULL,
            count INTEGER NOT NULL,
            recorded_at TEXT NOT NULL
        );",
    )
    .await?;

    execute_query(
        pool,
        "CREATE TABLE IF NOT EXISTS traffic_stats (
            host_id TEXT NOT NULL,
            period TEXT NOT NULL,
            operation TEXT NOT NULL,
            business TEXT NOT NULL,
            direction TEXT NOT NULL,
            total_bytes INTEGER NOT NULL,
            total_count INTEGER NOT NULL,
            PRIMARY KEY (host_id, period, operation, business, direction)
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
        "CREATE INDEX IF NOT EXISTS idx_files_host_id ON files(host_id);",
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

    // -- Traffic indexes ----------------------------------------------------

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_traffic_log_host_time ON traffic_log(host_id, recorded_at);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_traffic_log_business ON traffic_log(business, recorded_at);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_traffic_file_host_key ON traffic_file_log(host_id, file_key);",
    )
    .await?;

    execute_query(
        pool,
        "CREATE INDEX IF NOT EXISTS idx_traffic_file_time ON traffic_file_log(recorded_at);",
    )
    .await?;

    // -- Schema version ---------------------------------------------------

    // Set the schema version to 1.  Use INSERT OR IGNORE so that re-running
    // the migration does not fail if a row already exists.
    sqlx::query(
        "INSERT OR IGNORE INTO scan_metadata (host_id, db_schema_version) VALUES ('default', 1);",
    )
    .execute(pool)
    .await
    .map_err(|e| S3GalleryError::MigrationError(format!("Failed to set schema version: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_run_migrations_creates_tables() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
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
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let expected_tables = [
            "classification_rules",
            "dir_sizes",
            "extractor_rules",
            "file_tags",
            "files",
            "host_config",
            "metadata",
            "scan_metadata",
            "tags",
            "thumbnails",
            "traffic_file_log",
            "traffic_log",
            "traffic_stats",
        ];

        for name in &expected_tables {
            assert!(
                tables.contains(&name.to_string()),
                "table {name} should exist"
            );
        }

        assert_eq!(tables.len(), expected_tables.len());

        dir.close()
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        Ok(())
    }

    #[tokio::test]
    async fn test_migration_is_idempotent() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;

        // Run migrations twice
        run_migrations(&pool).await?;
        run_migrations(&pool).await?;

        // Verify tables still exist
        let tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .fetch_all(&pool)
                .await
                .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"metadata".to_string()));

        dir.close()
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        Ok(())
    }

    #[tokio::test]
    async fn test_all_indexes_created() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let indexes: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='index' AND name IS NOT NULL ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let expected_indexes = [
            "idx_files_file_type",
            "idx_files_host_id",
            "idx_files_last_modified",
            "idx_metadata_file_key",
            "idx_metadata_key_value",
            "idx_metadata_namespace",
            "idx_tags_tag_type",
            "idx_thumbnails_cached_at",
            "idx_traffic_file_host_key",
            "idx_traffic_file_time",
            "idx_traffic_log_business",
            "idx_traffic_log_host_time",
        ];

        for name in &expected_indexes {
            assert!(
                indexes.contains(&name.to_string()),
                "index {name} should exist"
            );
        }

        dir.close()
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        Ok(())
    }

    #[tokio::test]
    async fn test_schema_version_set() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let version: i64 =
            sqlx::query_scalar("SELECT db_schema_version FROM scan_metadata LIMIT 1")
                .fetch_one(&pool)
                .await
                .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        assert_eq!(version, 1);

        dir.close()
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        Ok(())
    }

    #[tokio::test]
    async fn test_foreign_keys_enabled() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        // PRAGMA foreign_keys returns 0 or 1.
        let fk_enabled: i32 = sqlx::query_scalar("PRAGMA foreign_keys;")
            .fetch_one(&pool)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        assert_eq!(fk_enabled, 1, "foreign keys should be enabled");

        dir.close()
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        Ok(())
    }
}
