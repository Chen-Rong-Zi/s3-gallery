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

    async fn seed_test_files(db: &DatabaseConnection) -> Result<()> {
        let pool = db.get_sqlite_connection_pool();
        sqlx::query(
            "INSERT OR REPLACE INTO files (host_id, key, etag, size, last_modified, content_type, file_type, metadata_state, effective_date, is_deleted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("test-host").bind("vacation/photo001.jpg").bind("\"abc123\"")
        .bind(1024i64).bind("2024-01-01T00:00:00Z").bind(Some("image/jpeg"))
        .bind("jpeg").bind("pending").bind("").bind(false)
        .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query(
            "INSERT OR REPLACE INTO files (host_id, key, etag, size, last_modified, content_type, file_type, metadata_state, effective_date, is_deleted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("test-host").bind("work/report.pdf").bind("\"def456\"")
        .bind(2048i64).bind("2024-01-02T00:00:00Z").bind(Some("application/pdf"))
        .bind("pdf").bind("pending").bind("").bind(false)
        .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query(
            "INSERT OR REPLACE INTO files (host_id, key, etag, size, last_modified, content_type, file_type, metadata_state, effective_date, is_deleted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("test-host").bind("family/portrait.jpg").bind("\"ghi789\"")
        .bind(4096i64).bind("2024-02-01T00:00:00Z").bind(Some("image/jpeg"))
        .bind("jpeg").bind("pending").bind("").bind(false)
        .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        Ok(())
    }

    async fn seed_test_tags(db: &DatabaseConnection) -> Result<()> {
        let pool = db.get_sqlite_connection_pool();

        sqlx::query("INSERT INTO tags (tag_name, tag_type) VALUES (?, ?)")
            .bind("photo").bind("auto")
            .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let tag_id: (i64,) = sqlx::query_as("SELECT tag_id FROM tags WHERE tag_name = ?")
            .bind("photo")
            .fetch_one(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query("INSERT OR IGNORE INTO file_tags (file_key, tag_id) VALUES (?, ?)")
            .bind("vacation/photo001.jpg").bind(tag_id.0)
            .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query("INSERT OR IGNORE INTO file_tags (file_key, tag_id) VALUES (?, ?)")
            .bind("family/portrait.jpg").bind(tag_id.0)
            .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

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