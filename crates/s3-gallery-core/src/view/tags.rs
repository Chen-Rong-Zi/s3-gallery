//! Tag operations.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::db::models::{FileEntry, TagEntry};
use crate::error::Result;
use crate::error::S3GalleryError;

/// List all tags.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn list_tags(
    db: &DatabaseConnection,
    host_id: Option<&str>,
) -> Result<Vec<TagEntry>> {
    let tags: Vec<TagEntry> = if let Some(hid) = host_id {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT DISTINCT t.* FROM tags t
                 INNER JOIN file_tags ft ON t.tag_id = ft.tag_id
                 INNER JOIN files f ON ft.file_key = f.key
                 WHERE f.host_id = ?
                 ORDER BY t.tag_name",
                [hid.into()],
            ))
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        rows_to_tag_entries(&rows)?
    } else {
        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT DISTINCT t.* FROM tags t
                 INNER JOIN file_tags ft ON t.tag_id = ft.tag_id
                 INNER JOIN files f ON ft.file_key = f.key
                 WHERE f.is_deleted = 0
                 ORDER BY t.tag_name"
                    .to_string(),
            ))
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        rows_to_tag_entries(&rows)?
    };

    Ok(tags)
}

/// Get files for a tag.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn get_files_by_tag(
    db: &DatabaseConnection,
    host_id: Option<&str>,
    tag_name: &str,
) -> Result<Vec<FileEntry>> {
    let files: Vec<FileEntry> = if let Some(hid) = host_id {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT f.* FROM files f
                 INNER JOIN file_tags ft ON f.key = ft.file_key
                 INNER JOIN tags t ON ft.tag_id = t.tag_id
                 WHERE t.tag_name = ? AND f.host_id = ? AND f.is_deleted = 0
                 ORDER BY f.key",
                [tag_name.into(), hid.into()],
            ))
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        rows_to_file_entries(&rows)?
    } else {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT f.* FROM files f
                 INNER JOIN file_tags ft ON f.key = ft.file_key
                 INNER JOIN tags t ON ft.tag_id = t.tag_id
                 WHERE t.tag_name = ? AND f.is_deleted = 0
                 ORDER BY f.key",
                [tag_name.into()],
            ))
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        rows_to_file_entries(&rows)?
    };

    Ok(files)
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

/// Convert query result rows to `Vec<TagEntry>`.
fn rows_to_tag_entries(rows: &[sea_orm::QueryResult]) -> Result<Vec<TagEntry>> {
    rows.iter()
        .map(|row| {
            Ok(TagEntry {
                tag_id: row
                    .try_get::<i64>("", "tag_id")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                tag_name: row
                    .try_get::<String>("", "tag_name")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
                tag_type: row
                    .try_get::<String>("", "tag_type")
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?,
            })
        })
        .collect::<std::result::Result<Vec<_>, S3GalleryError>>()
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

    async fn seed_test_data(pool: &SqlitePool) -> Result<()> {
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

        TagEntry::insert(
            pool,
            &TagEntry {
                tag_id: 0,
                tag_name: "photo".to_string(),
                tag_type: "auto".to_string(),
            },
        )
        .await?;

        TagEntry::insert(
            pool,
            &TagEntry {
                tag_id: 0,
                tag_name: "video".to_string(),
                tag_type: "auto".to_string(),
            },
        )
        .await?;

        let photo_tag = TagEntry::get_by_name(pool, "photo").await?;
        let video_tag = TagEntry::get_by_name(pool, "video").await?;

        FileTagEntry::insert(
            pool,
            &FileTagEntry {
                file_key: "photo001.jpg".to_string(),
                tag_id: photo_tag.tag_id,
            },
        )
        .await?;

        FileTagEntry::insert(
            pool,
            &FileTagEntry {
                file_key: "video.mp4".to_string(),
                tag_id: video_tag.tag_id,
            },
        )
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_list_tags() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_data(&pool).await?;

        let tags = list_tags(&pool, Some("test-host")).await?;
        assert_eq!(tags.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_files_by_tag() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_data(&pool).await?;

        let files = get_files_by_tag(&pool, Some("test-host"), "photo").await?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].key, "photo001.jpg");

        let files = get_files_by_tag(&pool, Some("test-host"), "video").await?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].key, "video.mp4");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_files_by_tag_nonexistent() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_data(&pool).await?;

        let files = get_files_by_tag(&pool, Some("test-host"), "nonexistent").await?;
        assert!(files.is_empty());

        Ok(())
    }
}
