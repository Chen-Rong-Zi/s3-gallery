//! File statistics functionality.

use std::collections::HashMap;
use std::str::FromStr;

use sqlx::SqlitePool;

use crate::db::models::FileEntry;
use crate::error::Result;
use crate::types::FileSize;

/// Statistics about files in the database.
#[derive(Debug, Clone)]
pub struct FileStats {
    /// Total number of non-deleted files.
    pub total_files: u64,
    /// Total size of all non-deleted files.
    pub total_size: FileSize,
    /// Count of files by category.
    pub by_category: HashMap<String, u64>,
    /// Count of files by file type.
    pub by_file_type: HashMap<String, u64>,
    /// Number of deleted files.
    pub deleted_files: u64,
    /// Number of files with extracted metadata.
    pub metadata_extracted: u64,
    /// Number of files with pending metadata extraction.
    pub metadata_pending: u64,
}

/// Get file statistics.
///
/// # Errors
///
/// Returns an error if any database query fails.
pub async fn get_stats(db: &SqlitePool) -> Result<FileStats> {
    let mut by_category = HashMap::new();
    let mut by_file_type = HashMap::new();

    let total_files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE is_deleted = 0")
        .fetch_one(db)
        .await
        .map_err(|e| crate::error::OssgalleyError::DbError(e.to_string()))?;

    let total_size: Option<i64> =
        sqlx::query_scalar("SELECT SUM(size) FROM files WHERE is_deleted = 0")
            .fetch_one(db)
            .await
            .map_err(|e| crate::error::OssgalleyError::DbError(e.to_string()))?;

    let deleted_files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE is_deleted = 1")
        .fetch_one(db)
        .await
        .map_err(|e| crate::error::OssgalleyError::DbError(e.to_string()))?;

    let metadata_extracted: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE is_deleted = 0 AND metadata_state = 'extracted'")
            .fetch_one(db)
            .await
            .map_err(|e| crate::error::OssgalleyError::DbError(e.to_string()))?;

    let metadata_pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE is_deleted = 0 AND metadata_state = 'pending'")
            .fetch_one(db)
            .await
            .map_err(|e| crate::error::OssgalleyError::DbError(e.to_string()))?;

    let files: Vec<FileEntry> = sqlx::query_as(
        "SELECT * FROM files WHERE is_deleted = 0"
    )
    .fetch_all(db)
    .await
    .map_err(|e| crate::error::OssgalleyError::DbError(e.to_string()))?;

    for file in files {
        *by_file_type.entry(file.file_type.clone()).or_insert(0) += 1;

        let file_type = crate::types::FileType::from_str(&file.file_type).unwrap_or(crate::types::FileType::Unknown);
        let category = file_type.category();
        *by_category
            .entry(category.to_string())
            .or_insert(0) += 1;
    }

    let total_size_u64 = u64::try_from(total_size.unwrap_or(0)).unwrap_or(0);

    Ok(FileStats {
        total_files: u64::try_from(total_files).unwrap_or(0),
        total_size: FileSize::new(total_size_u64),
        by_category,
        by_file_type,
        deleted_files: u64::try_from(deleted_files).unwrap_or(0),
        metadata_extracted: u64::try_from(metadata_extracted).unwrap_or(0),
        metadata_pending: u64::try_from(metadata_pending).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::FileEntry;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::OssgalleyError;
    use tempfile::tempdir;

    async fn setup_test_db() -> Result<(SqlitePool, tempfile::TempDir)> {
        let dir = tempdir().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;
        Ok((pool, dir))
    }

    async fn seed_test_files(pool: &SqlitePool) -> Result<()> {
        FileEntry::upsert(
            pool,
            &FileEntry {
                key: "img001.jpg".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 1024,
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "extracted".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                key: "img002.png".to_string(),
                etag: "\"def456\"".to_string(),
                size: 2048,
                last_modified: "2024-01-02T00:00:00Z".to_string(),
                content_type: Some("image/png".to_string()),
                file_type: "png".to_string(),
                metadata_state: "pending".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                key: "video.mp4".to_string(),
                etag: "\"ghi789\"".to_string(),
                size: 50000,
                last_modified: "2024-02-01T00:00:00Z".to_string(),
                content_type: Some("video/mp4".to_string()),
                file_type: "mp4".to_string(),
                metadata_state: "pending".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                key: "deleted.txt".to_string(),
                etag: "\"jkl012\"".to_string(),
                size: 50,
                last_modified: "2024-03-01T00:00:00Z".to_string(),
                content_type: None,
                file_type: "unknown".to_string(),
                metadata_state: "pending".to_string(),
                is_deleted: true,
            },
        )
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_get_stats() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let stats = get_stats(&pool).await?;

        assert_eq!(stats.total_files, 3);
        assert_eq!(stats.total_size.as_u64(), 1024 + 2048 + 50000);
        assert_eq!(stats.deleted_files, 1);
        assert_eq!(stats.metadata_extracted, 1);
        assert_eq!(stats.metadata_pending, 2);

        assert_eq!(stats.by_file_type.get("jpeg"), Some(&1));
        assert_eq!(stats.by_file_type.get("png"), Some(&1));
        assert_eq!(stats.by_file_type.get("mp4"), Some(&1));

        assert_eq!(stats.by_category.get("image"), Some(&2));
        assert_eq!(stats.by_category.get("video"), Some(&1));

        Ok(())
    }

    #[tokio::test]
    async fn test_get_stats_empty() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        let stats = get_stats(&pool).await?;

        assert_eq!(stats.total_files, 0);
        assert_eq!(stats.total_size.as_u64(), 0);
        assert_eq!(stats.deleted_files, 0);
        assert_eq!(stats.metadata_extracted, 0);
        assert_eq!(stats.metadata_pending, 0);

        Ok(())
    }
}
