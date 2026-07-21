//! Tag operations.

use sqlx::SqlitePool;

use crate::db::models::{FileEntry, TagEntry};
use crate::error::Result;

/// List all tags.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn list_tags(db: &SqlitePool) -> Result<Vec<TagEntry>> {
    TagEntry::list_all(db).await
}

/// Get files for a tag.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn get_files_by_tag(db: &SqlitePool, tag_name: &str) -> Result<Vec<FileEntry>> {
    let files: Vec<FileEntry> = sqlx::query_as(
        "SELECT f.* FROM files f
         INNER JOIN file_tags ft ON f.key = ft.file_key
         INNER JOIN tags t ON ft.tag_id = t.tag_id
         WHERE t.tag_name = ? AND f.is_deleted = 0
         ORDER BY f.key"
    )
    .bind(tag_name)
    .fetch_all(db)
    .await
    .map_err(|e| crate::error::OssgalleyError::DbError(e.to_string()))?;

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{FileEntry, FileTagEntry, TagEntry};
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

    async fn seed_test_data(pool: &SqlitePool) -> Result<()> {
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

        let tags = list_tags(&pool).await?;
        assert_eq!(tags.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_files_by_tag() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_data(&pool).await?;

        let files = get_files_by_tag(&pool, "photo").await?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].key, "photo001.jpg");

        let files = get_files_by_tag(&pool, "video").await?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].key, "video.mp4");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_files_by_tag_nonexistent() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_data(&pool).await?;

        let files = get_files_by_tag(&pool, "nonexistent").await?;
        assert!(files.is_empty());

        Ok(())
    }
}
