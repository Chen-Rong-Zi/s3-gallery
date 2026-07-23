//! File statistics functionality.

use std::collections::HashMap;
use std::str::FromStr;

use sqlx::SqlitePool;

use crate::db::models::FileEntry;
use crate::error::Result;
use crate::types::FileSize;
use crate::util::db_helpers::{fetch_all_opt, fetch_scalar_opt};

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
pub async fn get_stats(db: &SqlitePool, host_id: Option<&str>) -> Result<FileStats> {
    let mut by_category = HashMap::new();
    let mut by_file_type = HashMap::new();

    let total_files: i64 = fetch_scalar_opt(
        db,
        "SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0",
        host_id,
    )
    .await?;

    let total_size: Option<i64> = fetch_scalar_opt(
        db,
        "SELECT SUM(size) FROM files WHERE host_id = ? AND is_deleted = 0",
        host_id,
    )
    .await?;

    let deleted_files: i64 = fetch_scalar_opt(
        db,
        "SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 1",
        host_id,
    )
    .await?;

    let metadata_extracted: i64 = fetch_scalar_opt(
        db,
        "SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0 AND metadata_state = 'extracted'",
        host_id,
    )
    .await?;

    let metadata_pending: i64 = fetch_scalar_opt(
        db,
        "SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0 AND metadata_state = 'pending'",
        host_id,
    )
    .await?;

    let files: Vec<FileEntry> = fetch_all_opt(
        db,
        "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0",
        host_id,
    )
    .await?;

    for file in files {
        *by_file_type.entry(file.file_type.clone()).or_insert(0) += 1;

        let file_type = crate::types::FileType::from_str(&file.file_type)
            .unwrap_or(crate::types::FileType::Unknown);
        let category = file_type.category();
        *by_category.entry(category.to_string()).or_insert(0) += 1;
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

/// Get per-host statistics and total aggregate.
///
/// Returns `(total_stats, vec_of_(host_id, stats))`.
///
/// # Errors
///
/// Returns an error if any database query fails.
pub async fn get_all_host_stats(db: &SqlitePool) -> Result<(FileStats, Vec<(String, FileStats)>)> {
    let total = get_stats(db, None).await?;

    let host_ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT host_id FROM files WHERE is_deleted = 0 ORDER BY host_id",
    )
    .fetch_all(db)
    .await
    .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?;

    let mut per_host = Vec::with_capacity(host_ids.len());
    for hid in &host_ids {
        let stats = get_stats(db, Some(hid)).await?;
        per_host.push((hid.clone(), stats));
    }

    Ok((total, per_host))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::FileEntry;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    async fn setup_test_db() -> Result<(SqlitePool, tempfile::TempDir)> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;
        Ok((pool, dir))
    }

    async fn seed_test_files(pool: &SqlitePool) -> Result<()> {
        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "test-host".to_string(),
                key: "img001.jpg".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 1024,
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "extracted".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "test-host".to_string(),
                key: "img002.png".to_string(),
                etag: "\"def456\"".to_string(),
                size: 2048,
                last_modified: "2024-01-02T00:00:00Z".to_string(),
                content_type: Some("image/png".to_string()),
                file_type: "png".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "test-host".to_string(),
                key: "video.mp4".to_string(),
                etag: "\"ghi789\"".to_string(),
                size: 50000,
                last_modified: "2024-02-01T00:00:00Z".to_string(),
                content_type: Some("video/mp4".to_string()),
                file_type: "mp4".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "test-host".to_string(),
                key: "deleted.txt".to_string(),
                etag: "\"jkl012\"".to_string(),
                size: 50,
                last_modified: "2024-03-01T00:00:00Z".to_string(),
                content_type: None,
                file_type: "unknown".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
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

        let stats = get_stats(&pool, Some("test-host")).await?;

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

        let stats = get_stats(&pool, Some("test-host")).await?;

        assert_eq!(stats.total_files, 0);
        assert_eq!(stats.total_size.as_u64(), 0);
        assert_eq!(stats.deleted_files, 0);
        assert_eq!(stats.metadata_extracted, 0);
        assert_eq!(stats.metadata_pending, 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_all_host_stats() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let (total, per_host) = get_all_host_stats(&pool).await?;

        assert_eq!(total.total_files, 3);
        assert_eq!(total.deleted_files, 1);
        assert_eq!(per_host.len(), 1);
        assert_eq!(per_host[0].0, "test-host");
        assert_eq!(per_host[0].1.total_files, 3);

        Ok(())
    }
}
