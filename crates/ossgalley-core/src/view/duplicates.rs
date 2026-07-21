//! Find duplicate files.

use sqlx::SqlitePool;

use crate::db::models::FileEntry;
use crate::error::Result;
use crate::types::FileSize;

/// A group of duplicate files.
#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    /// Size of the files in this group.
    pub size: FileSize,
    /// Files in this duplicate group.
    pub files: Vec<FileEntry>,
}

/// Find duplicate files (same size + same etag).
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn find_duplicates(db: &SqlitePool) -> Result<Vec<DuplicateGroup>> {
    #[derive(Debug, sqlx::FromRow)]
    struct DuplicateKey {
        size: i64,
        etag: String,
    }

    let keys: Vec<DuplicateKey> = sqlx::query_as(
        "SELECT size, etag
         FROM files
         WHERE is_deleted = 0
         GROUP BY size, etag
         HAVING COUNT(*) > 1
         ORDER BY size DESC"
    )
    .fetch_all(db)
    .await
    .map_err(|e| crate::error::OssgalleyError::DbError(e.to_string()))?;

    let mut result = Vec::new();

    for key in keys {
        let files: Vec<FileEntry> = sqlx::query_as(
            "SELECT * FROM files
             WHERE size = ? AND etag = ? AND is_deleted = 0
             ORDER BY key"
        )
        .bind(key.size)
        .bind(&key.etag)
        .fetch_all(db)
        .await
        .map_err(|e| crate::error::OssgalleyError::DbError(e.to_string()))?;

        let size = u64::try_from(key.size).unwrap_or(0);

        result.push(DuplicateGroup {
            size: FileSize::new(size),
            files,
        });
    }

    Ok(result)
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
                key: "photo001.jpg".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 1024,
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "pending".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                key: "backup/photo001.jpg".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 1024,
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "pending".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                key: "video.mp4".to_string(),
                etag: "\"def456\"".to_string(),
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
                key: "video_backup.mp4".to_string(),
                etag: "\"def456\"".to_string(),
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
                key: "unique.txt".to_string(),
                etag: "\"ghi789\"".to_string(),
                size: 50,
                last_modified: "2024-03-01T00:00:00Z".to_string(),
                content_type: None,
                file_type: "unknown".to_string(),
                metadata_state: "pending".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_find_duplicates() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let duplicates = find_duplicates(&pool).await?;
        assert_eq!(duplicates.len(), 2);

        assert_eq!(duplicates[0].size.as_u64(), 50000);
        assert_eq!(duplicates[0].files.len(), 2);

        assert_eq!(duplicates[1].size.as_u64(), 1024);
        assert_eq!(duplicates[1].files.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_find_duplicates_none() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        FileEntry::upsert(
            &pool,
            &FileEntry {
                key: "unique1.txt".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 50,
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                content_type: None,
                file_type: "unknown".to_string(),
                metadata_state: "pending".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            &pool,
            &FileEntry {
                key: "unique2.txt".to_string(),
                etag: "\"def456\"".to_string(),
                size: 60,
                last_modified: "2024-01-02T00:00:00Z".to_string(),
                content_type: None,
                file_type: "unknown".to_string(),
                metadata_state: "pending".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        let duplicates = find_duplicates(&pool).await?;
        assert!(duplicates.is_empty());

        Ok(())
    }
}
