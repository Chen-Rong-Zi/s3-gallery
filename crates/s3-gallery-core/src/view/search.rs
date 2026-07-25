//! Search functionality.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};

use crate::entity::{file, file_tag, tag};
use crate::error::Result;
use crate::error::S3GalleryError;

/// Search results containing matching files.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Matching files.
    pub files: Vec<file::Model>,
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
    let s = file::Entity::find()
        .filter(file::Column::IsDeleted.eq(false))
        .filter(file::Column::HostId.eq(host_id))
        .filter(file::Column::Key.contains(query));

    let files = s
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

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
    let files = file::Entity::find()
        .join_rev(JoinType::InnerJoin, file_tag::Relation::File.def())
        .join(JoinType::InnerJoin, file_tag::Relation::Tag.def())
        .filter(tag::Column::TagName.eq(tag_name))
        .filter(file::Column::HostId.eq(host_id))
        .filter(file::Column::IsDeleted.eq(false))
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let total_count = u64::try_from(files.len()).unwrap_or(0);

    Ok(SearchResult { files, total_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::FileEntry;
    use crate::db::models::{FileTagEntry, TagEntry};
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

    async fn seed_test_files(db: &DatabaseConnection) -> Result<()> {
        let pool = db.get_sqlite_connection_pool();
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

    async fn seed_test_tags(db: &DatabaseConnection) -> Result<()> {
        let pool = db.get_sqlite_connection_pool();
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
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let result = search_by_name(&db, "test-host", "jpg").await?;
        assert_eq!(result.total_count, 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_search_by_name_partial() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let result = search_by_name(&db, "test-host", "vacation").await?;
        assert_eq!(result.total_count, 1);
        assert_eq!(result.files[0].key.as_str(), "vacation/photo001.jpg");

        Ok(())
    }

    #[tokio::test]
    async fn test_search_by_name_no_match() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let result = search_by_name(&db, "test-host", "nonexistent").await?;
        assert_eq!(result.total_count, 0);
        assert!(result.files.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_search_by_tag() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;
        seed_test_tags(&db).await?;

        let result = search_by_tag(&db, "test-host", "photo").await?;
        assert_eq!(result.total_count, 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_search_by_tag_no_match() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let result = search_by_tag(&db, "test-host", "nonexistent").await?;
        assert_eq!(result.total_count, 0);
        assert!(result.files.is_empty());

        Ok(())
    }
}