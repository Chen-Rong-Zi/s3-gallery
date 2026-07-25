//! Find duplicate files.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::db::models::FileEntry;
use crate::error::Result;
use crate::error::S3GalleryError;
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
pub async fn find_duplicates(
    db: &DatabaseConnection,
    host_id: Option<&str>,
) -> Result<Vec<DuplicateGroup>> {
    // Query for duplicate keys (size, etag pairs with count > 1)
    let keys_sql = if host_id.is_some() {
        "SELECT size, etag FROM files \
         WHERE host_id = ? AND is_deleted = 0 \
         GROUP BY size, etag HAVING COUNT(*) > 1 ORDER BY size DESC"
    } else {
        "SELECT size, etag FROM files \
         WHERE is_deleted = 0 \
         GROUP BY size, etag HAVING COUNT(*) > 1 ORDER BY size DESC"
    };

    let key_rows = if let Some(hid) = host_id {
        db.query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            keys_sql,
            [hid.into()],
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    } else {
        db.query_all(Statement::from_string(DbBackend::Sqlite, keys_sql.to_string()))
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    };

    let mut result = Vec::new();

    for key_row in &key_rows {
        let dup_size: i64 = key_row
            .try_get("", "size")
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let dup_etag: String = key_row
            .try_get("", "etag")
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let files = if let Some(hid) = host_id {
            let rows = db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT * FROM files
                     WHERE host_id = ? AND size = ? AND etag = ? AND is_deleted = 0
                     ORDER BY key",
                    [hid.into(), dup_size.into(), dup_etag.into()],
                ))
                .await
                .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
            rows_to_file_entries(&rows)?
        } else {
            let rows = db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT * FROM files
                     WHERE size = ? AND etag = ? AND is_deleted = 0
                     ORDER BY key",
                    [dup_size.into(), dup_etag.into()],
                ))
                .await
                .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
            rows_to_file_entries(&rows)?
        };

        let size = u64::try_from(dup_size).unwrap_or(0);

        result.push(DuplicateGroup {
            size: FileSize::new(size),
            files,
        });
    }

    Ok(result)
}

/// Convert query result rows to `Vec<FileEntry>`.
fn rows_to_file_entries(rows: &[sea_orm::QueryResult]) -> Result<Vec<FileEntry>> {
    rows.iter()
        .map(|row| {
            Ok(FileEntry {
                host_id: row
                    .try_get::<String>("", "host_id")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                key: row
                    .try_get::<String>("", "key")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                etag: row
                    .try_get::<String>("", "etag")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                size: row
                    .try_get::<i64>("", "size")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                last_modified: row
                    .try_get::<String>("", "last_modified")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                content_type: row
                    .try_get::<Option<String>>("", "content_type")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                file_type: row
                    .try_get::<String>("", "file_type")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                metadata_state: row
                    .try_get::<String>("", "metadata_state")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                effective_date: row
                    .try_get::<String>("", "effective_date")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                is_deleted: row
                    .try_get::<bool>("", "is_deleted")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
            })
        })
        .collect::<std::result::Result<Vec<_>, S3GalleryError>>()
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
                key: "photo001.jpg".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 1024,
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
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
                key: "backup/photo001.jpg".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 1024,
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
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
                etag: "\"def456\"".to_string(),
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
                key: "video_backup.mp4".to_string(),
                etag: "\"def456\"".to_string(),
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
                key: "unique.txt".to_string(),
                etag: "\"ghi789\"".to_string(),
                size: 50,
                last_modified: "2024-03-01T00:00:00Z".to_string(),
                content_type: None,
                file_type: "unknown".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
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

        let duplicates = find_duplicates(&pool, Some("test-host")).await?;
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
                host_id: "test-host".to_string(),
                key: "unique1.txt".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 50,
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                content_type: None,
                file_type: "unknown".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            &pool,
            &FileEntry {
                host_id: "test-host".to_string(),
                key: "unique2.txt".to_string(),
                etag: "\"def456\"".to_string(),
                size: 60,
                last_modified: "2024-01-02T00:00:00Z".to_string(),
                content_type: None,
                file_type: "unknown".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        let duplicates = find_duplicates(&pool, Some("test-host")).await?;
        assert!(duplicates.is_empty());

        Ok(())
    }
}
