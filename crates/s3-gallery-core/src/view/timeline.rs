//! Timeline functionality - files grouped by date.

use std::collections::BTreeMap;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, Order, QueryFilter, QueryOrder};

use crate::entity::file;
use crate::error::Result;
use crate::error::S3GalleryError;

/// A timeline entry containing files from a specific date.
#[derive(Debug, Clone)]
pub struct TimelineEntry {
    /// Date in YYYY-MM-DD format.
    pub date: String,
    /// Files from this date.
    pub files: Vec<file::Model>,
    /// Number of files in this entry.
    pub count: u64,
}

/// Get timeline grouped by date.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn get_timeline(
    db: &DatabaseConnection,
    host_id: Option<&str>,
) -> Result<Vec<TimelineEntry>> {
    let mut query = file::Entity::find()
        .filter(file::Column::IsDeleted.eq(false))
        .order_by(file::Column::LastModified, Order::Desc);

    if let Some(hid) = host_id {
        query = query.filter(file::Column::HostId.eq(hid));
    }

    let files = query
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let mut grouped: BTreeMap<String, Vec<file::Model>> = BTreeMap::new();

    for file in files {
        let date = if file.effective_date.is_empty() {
            extract_date(&file.last_modified)
        } else {
            file.effective_date.clone()
        };
        grouped.entry(date).or_default().push(file);
    }

    let mut result = Vec::new();

    for (date, mut files) in grouped.into_iter().rev() {
        let count = u64::try_from(files.len()).unwrap_or(0);
        files.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
        result.push(TimelineEntry { date, files, count });
    }

    Ok(result)
}

fn extract_date(timestamp: &str) -> String {
    if timestamp.len() >= 10 {
        timestamp[..10].to_string()
    } else {
        timestamp.to_string()
    }
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
                key: "img001.jpg".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 1024,
                last_modified: "2024-01-15T10:30:00Z".to_string(),
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
                key: "img002.jpg".to_string(),
                etag: "\"def456\"".to_string(),
                size: 2048,
                last_modified: "2024-01-15T11:00:00Z".to_string(),
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
                etag: "\"ghi789\"".to_string(),
                size: 50000,
                last_modified: "2024-02-20T14:00:00Z".to_string(),
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
                key: "deleted.txt".to_string(),
                etag: "\"jkl012\"".to_string(),
                size: 50,
                last_modified: "2024-03-01T00:00:00Z".to_string(),
                content_type: None,
                file_type: "unknown".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: true,
            },
        )
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_get_timeline() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let timeline = get_timeline(&db, Some("test-host")).await?;
        assert_eq!(timeline.len(), 2);

        assert_eq!(timeline[0].date, "2024-02-20");
        assert_eq!(timeline[0].count, 1);

        assert_eq!(timeline[1].date, "2024-01-15");
        assert_eq!(timeline[1].count, 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_timeline_empty() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;

        let timeline = get_timeline(&db, Some("test-host")).await?;
        assert!(timeline.is_empty());

        Ok(())
    }
}