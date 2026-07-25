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
    use crate::db::models::{FileEntry, FileTagEntry, TagEntry};
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    async fn setup_test_db() -> Result<(DatabaseConnection, tempfile::TempDir)> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let db = create_pool(&db_path).await?;
        let pool = db.get_sqlite_connection_pool();
        run_migrations(pool).await?;
        Ok((db, dir))
    }

    async fn seed_test_data(db: &DatabaseConnection) -> Result<()> {
        let pool = db.get_sqlite_connection_pool();
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