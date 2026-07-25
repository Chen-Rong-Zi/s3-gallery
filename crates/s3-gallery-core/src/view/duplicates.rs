//! Find duplicate files.

use sea_orm::{ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, Order, QueryFilter, QueryOrder};

use crate::entity::file;
use crate::error::Result;
use crate::error::S3GalleryError;
use crate::types::FileSize;

/// A group of duplicate files.
#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    /// Size of the files in this group.
    pub size: FileSize,
    /// Files in this duplicate group.
    pub files: Vec<file::Model>,
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
    // We use raw SQL because GROUP BY with HAVING is not directly supported
    // by SeaORM's query builder.
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
        db.query_all(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            keys_sql,
            [hid.into()],
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    } else {
        db.query_all(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            keys_sql.to_string(),
        ))
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

        let mut query = file::Entity::find()
            .filter(file::Column::Size.eq(dup_size))
            .filter(file::Column::Etag.eq(dup_etag))
            .filter(file::Column::IsDeleted.eq(false))
            .order_by(file::Column::Key, Order::Asc);

        if let Some(hid) = host_id {
            query = query.filter(file::Column::HostId.eq(hid));
        }

        let files = query
            .all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let size = u64::try_from(dup_size).unwrap_or(0);

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
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let duplicates = find_duplicates(&db, Some("test-host")).await?;
        assert_eq!(duplicates.len(), 2);

        assert_eq!(duplicates[0].size.as_u64(), 50000);
        assert_eq!(duplicates[0].files.len(), 2);

        assert_eq!(duplicates[1].size.as_u64(), 1024);
        assert_eq!(duplicates[1].files.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_find_duplicates_none() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        let pool = db.get_sqlite_connection_pool();

        FileEntry::upsert(
            pool,
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
            pool,
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

        let duplicates = find_duplicates(&db, Some("test-host")).await?;
        assert!(duplicates.is_empty());

        Ok(())
    }
}