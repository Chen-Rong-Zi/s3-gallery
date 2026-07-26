//! Tag operations.

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, JoinType, QueryFilter, QuerySelect,
    RelationTrait,
};

use crate::entity::{file, file_tag, tag};
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
) -> Result<Vec<tag::Model>> {
    let tags: Vec<tag::Model> = if let Some(hid) = host_id {
        tag::Entity::find()
            .distinct()
            .join_rev(JoinType::InnerJoin, file_tag::Relation::Tag.def())
            .join(JoinType::InnerJoin, file_tag::Relation::File.def())
            .filter(file::Column::HostId.eq(hid))
            .filter(file::Column::IsDeleted.eq(false))
            .all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    } else {
        tag::Entity::find()
            .distinct()
            .join_rev(JoinType::InnerJoin, file_tag::Relation::Tag.def())
            .join(JoinType::InnerJoin, file_tag::Relation::File.def())
            .filter(file::Column::IsDeleted.eq(false))
            .all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
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
) -> Result<Vec<file::Model>> {
    let files: Vec<file::Model> = if let Some(hid) = host_id {
        file::Entity::find()
            .join_rev(JoinType::InnerJoin, file_tag::Relation::File.def())
            .join(JoinType::InnerJoin, file_tag::Relation::Tag.def())
            .filter(tag::Column::TagName.eq(tag_name))
            .filter(file::Column::HostId.eq(hid))
            .filter(file::Column::IsDeleted.eq(false))
            .all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    } else {
        file::Entity::find()
            .join_rev(JoinType::InnerJoin, file_tag::Relation::File.def())
            .join(JoinType::InnerJoin, file_tag::Relation::Tag.def())
            .filter(tag::Column::TagName.eq(tag_name))
            .filter(file::Column::IsDeleted.eq(false))
            .all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    };

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate::run_full_migration;
    use crate::db::pool::create_pool;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    async fn setup_test_db() -> Result<(DatabaseConnection, tempfile::TempDir)> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let db = create_pool(&db_path).await?;
        run_full_migration(&db).await?;
        Ok((db, dir))
    }

    async fn seed_test_data(db: &DatabaseConnection) -> Result<()> {
        let pool = db.get_sqlite_connection_pool();
        sqlx::query(
            "INSERT OR REPLACE INTO files (host_id, key, etag, size, last_modified, content_type, file_type, metadata_state, effective_date, is_deleted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("test-host").bind("photo001.jpg").bind("\"abc123\"")
        .bind(1024i64).bind("2024-01-01T00:00:00Z").bind(Some("image/jpeg"))
        .bind("jpeg").bind("pending").bind("").bind(false)
        .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query(
            "INSERT OR REPLACE INTO files (host_id, key, etag, size, last_modified, content_type, file_type, metadata_state, effective_date, is_deleted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("test-host").bind("video.mp4").bind("\"def456\"")
        .bind(50000i64).bind("2024-02-01T00:00:00Z").bind(Some("video/mp4"))
        .bind("mp4").bind("pending").bind("").bind(false)
        .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query("INSERT INTO tags (tag_name, tag_type) VALUES (?, ?)")
            .bind("photo").bind("auto")
            .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query("INSERT INTO tags (tag_name, tag_type) VALUES (?, ?)")
            .bind("video").bind("auto")
            .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let photo_id: (i64,) = sqlx::query_as("SELECT tag_id FROM tags WHERE tag_name = ?")
            .bind("photo")
            .fetch_one(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let video_id: (i64,) = sqlx::query_as("SELECT tag_id FROM tags WHERE tag_name = ?")
            .bind("video")
            .fetch_one(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query("INSERT OR IGNORE INTO file_tags (file_key, tag_id) VALUES (?, ?)")
            .bind("photo001.jpg").bind(photo_id.0)
            .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query("INSERT OR IGNORE INTO file_tags (file_key, tag_id) VALUES (?, ?)")
            .bind("video.mp4").bind(video_id.0)
            .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        Ok(())
    }

    #[tokio::test]
    async fn test_list_tags() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_data(&db).await?;

        let tags = list_tags(&db, Some("test-host")).await?;
        assert_eq!(tags.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_files_by_tag() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_data(&db).await?;

        let files = get_files_by_tag(&db, Some("test-host"), "photo").await?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].key.as_str(), "photo001.jpg");

        let files = get_files_by_tag(&db, Some("test-host"), "video").await?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].key.as_str(), "video.mp4");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_files_by_tag_nonexistent() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_data(&db).await?;

        let files = get_files_by_tag(&db, Some("test-host"), "nonexistent").await?;
        assert!(files.is_empty());

        Ok(())
    }
}