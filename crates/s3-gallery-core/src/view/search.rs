//! Search functionality.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::db::models::FileEntry;
use crate::error::Result;
use crate::error::S3GalleryError;

/// Search results containing matching files.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Matching files.
    pub files: Vec<FileEntry>,
    /// Total count of matching files.
    pub total_count: u64,
}

/// Search files by name pattern.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn search_by_name(
    db: &DatabaseConnection,
    host_id: &str,
    query: &str,
) -> Result<SearchResult> {
    let pattern = format!("%{query}%");

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM files WHERE host_id = ? AND key LIKE ? AND is_deleted = 0 ORDER BY key",
            [host_id.into(), pattern.into()],
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let files = rows
        .iter()
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
        .collect::<std::result::Result<Vec<_>, S3GalleryError>>()?;

    let total_count = u64::try_from(files.len()).unwrap_or(0);

    Ok(SearchResult { files, total_count })
}

/// Search files by tag.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn search_by_tag(
    db: &DatabaseConnection,
    host_id: &str,
    tag_name: &str,
) -> Result<SearchResult> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT f.* FROM files f
             INNER JOIN file_tags ft ON f.key = ft.file_key
             INNER JOIN tags t ON ft.tag_id = t.tag_id
             WHERE t.tag_name = ? AND f.host_id = ? AND f.is_deleted = 0
             ORDER BY f.key",
            [tag_name.into(), host_id.into()],
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let files = rows
        .iter()
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
        .collect::<std::result::Result<Vec<_>, S3GalleryError>>()?;

    let total_count = u64::try_from(files.len()).unwrap_or(0);

    Ok(SearchResult { files, total_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{FileEntry, FileTagEntry, TagEntry};
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
                key: "vacation/photo001.jpg".to_string(),
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
                key: "work/report.pdf".to_string(),
                etag: "\"def456\"".to_string(),
                size: 2048,
                last_modified: "2024-01-02T00:00:00Z".to_string(),
                content_type: Some("application/pdf".to_string()),
                file_type: "pdf".to_string(),
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
                key: "family/portrait.jpg".to_string(),
                etag: "\"ghi789\"".to_string(),
                size: 4096,
                last_modified: "2024-02-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        Ok(())
    }

    async fn seed_test_tags(pool: &SqlitePool) -> Result<()> {
        TagEntry::insert(
            pool,
            &TagEntry {
                tag_id: 0,
                tag_name: "photo".to_string(),
                tag_type: "auto".to_string(),
            },
        )
        .await?;

        let tag = TagEntry::get_by_name(pool, "photo").await?;

        FileTagEntry::insert(
            pool,
            &FileTagEntry {
                file_key: "vacation/photo001.jpg".to_string(),
                tag_id: tag.tag_id,
            },
        )
        .await?;

        FileTagEntry::insert(
            pool,
            &FileTagEntry {
                file_key: "family/portrait.jpg".to_string(),
                tag_id: tag.tag_id,
            },
        )
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_search_by_name() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let result = search_by_name(&pool, "test-host", "jpg").await?;
        assert_eq!(result.total_count, 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_search_by_name_partial() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let result = search_by_name(&pool, "test-host", "vacation").await?;
        assert_eq!(result.total_count, 1);
        assert_eq!(result.files[0].key, "vacation/photo001.jpg");

        Ok(())
    }

    #[tokio::test]
    async fn test_search_by_name_no_match() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let result = search_by_name(&pool, "test-host", "nonexistent").await?;
        assert_eq!(result.total_count, 0);
        assert!(result.files.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_search_by_tag() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;
        seed_test_tags(&pool).await?;

        let result = search_by_tag(&pool, "test-host", "photo").await?;
        assert_eq!(result.total_count, 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_search_by_tag_no_match() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let result = search_by_tag(&pool, "test-host", "nonexistent").await?;
        assert_eq!(result.total_count, 0);
        assert!(result.files.is_empty());

        Ok(())
    }
}
