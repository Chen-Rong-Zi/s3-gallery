//! Unified timeline-gallery view — files grouped by date, tag-filterable, paginated.

use std::collections::BTreeMap;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};

use crate::entity::{file, file_tag, tag};
use crate::error::{Result, S3GalleryError};

/// A timeline entry containing files from a specific date.
#[derive(Debug, Clone)]
pub struct TimelineEntry {
    pub date: String,
    pub files: Vec<file::Model>,
    pub count: u64,
}

/// Get timeline-gallery entries grouped by date, with optional tag filtering.
///
/// Returns `(entries, has_more)` where `has_more` is true if more pages exist.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn get_timeline_gallery(
    db: &DatabaseConnection,
    host_id: Option<&str>,
    page: u32,
    page_size: u32,
    tag: Option<&str>,
) -> Result<(Vec<TimelineEntry>, bool)> {
    // Build the query with optional host_id and tag filters
    let files: Vec<file::Model> = if let Some(tag_name) = tag {
        if tag_name.is_empty() {
            return Ok((Vec::new(), false));
        }
        let mut query = file::Entity::find()
            .join_rev(JoinType::InnerJoin, file_tag::Relation::File.def())
            .join(JoinType::InnerJoin, file_tag::Relation::Tag.def())
            .filter(tag::Column::TagName.eq(tag_name))
            .filter(file::Column::IsDeleted.eq(false));

        if let Some(hid) = host_id {
            query = query.filter(file::Column::HostId.eq(hid));
        }

        query
            .all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    } else {
        let mut query = file::Entity::find()
            .filter(file::Column::IsDeleted.eq(false));

        if let Some(hid) = host_id {
            query = query.filter(file::Column::HostId.eq(hid));
        }

        query
            .all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
    };

    // Group by date (effective_date or last_modified)
    let mut grouped: BTreeMap<String, Vec<file::Model>> = BTreeMap::new();
    for file in files {
        let date = if file.effective_date.is_empty() {
            extract_date(&file.last_modified)
        } else {
            file.effective_date.clone()
        };
        grouped.entry(date).or_default().push(file);
    }

    // Sort dates descending (newest first), paginate by date groups
    let all_dates: Vec<String> = grouped.keys().cloned().rev().collect();
    let offset = page as usize * page_size as usize;
    let has_more = all_dates.len() > offset + page_size as usize;

    // Build entries from the already-fetched files (no re-query needed)
    let entries: Vec<TimelineEntry> = all_dates
        .iter()
        .skip(offset)
        .take(page_size as usize)
        .map(|date_str| {
            let files = grouped.remove(date_str).unwrap_or_default();
            let count = files.len() as u64;
            TimelineEntry {
                date: date_str.clone(),
                files,
                count,
            }
        })
        .collect();

    Ok((entries, has_more))
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
                host_id: "host1".into(),
                key: "a.jpg".into(),
                etag: "\"1\"".into(),
                size: 100,
                last_modified: "2024-01-15T10:00:00Z".into(),
                content_type: Some("image/jpeg".into()),
                file_type: "jpeg".into(),
                metadata_state: "pending".into(),
                effective_date: "".into(),
                is_deleted: false,
            },
        )
        .await?;
        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "host1".into(),
                key: "b.jpg".into(),
                etag: "\"2\"".into(),
                size: 200,
                last_modified: "2024-01-15T11:00:00Z".into(),
                content_type: Some("image/jpeg".into()),
                file_type: "jpeg".into(),
                metadata_state: "pending".into(),
                effective_date: "".into(),
                is_deleted: false,
            },
        )
        .await?;
        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "host1".into(),
                key: "c.mp4".into(),
                etag: "\"3\"".into(),
                size: 50000,
                last_modified: "2024-02-20T14:00:00Z".into(),
                content_type: Some("video/mp4".into()),
                file_type: "mp4".into(),
                metadata_state: "pending".into(),
                effective_date: "".into(),
                is_deleted: false,
            },
        )
        .await?;
        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "host1".into(),
                key: "d.txt".into(),
                etag: "\"4\"".into(),
                size: 50,
                last_modified: "2024-03-01T00:00:00Z".into(),
                content_type: None,
                file_type: "unknown".into(),
                metadata_state: "pending".into(),
                effective_date: "".into(),
                is_deleted: true,
            },
        )
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_get_timeline_gallery_basic() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let (entries, has_more) = get_timeline_gallery(&db, None, 0, 10, None).await?;
        assert_eq!(entries.len(), 2);
        assert!(!has_more);
        assert_eq!(entries[0].date, "2024-02-20");
        assert_eq!(entries[0].count, 1);
        assert_eq!(entries[1].date, "2024-01-15");
        assert_eq!(entries[1].count, 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_timeline_gallery_pagination() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let (entries, has_more) = get_timeline_gallery(&db, None, 0, 1, None).await?;
        assert_eq!(entries.len(), 1);
        assert!(has_more);
        assert_eq!(entries[0].date, "2024-02-20");

        let (entries, has_more) = get_timeline_gallery(&db, None, 1, 1, None).await?;
        assert_eq!(entries.len(), 1);
        assert!(!has_more);
        assert_eq!(entries[0].date, "2024-01-15");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_timeline_gallery_empty() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        let (entries, has_more) = get_timeline_gallery(&db, None, 0, 10, None).await?;
        assert!(entries.is_empty());
        assert!(!has_more);
        Ok(())
    }
}