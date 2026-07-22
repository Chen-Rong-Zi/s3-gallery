//! Unified timeline-gallery view — files grouped by date, tag-filterable, paginated.

use std::collections::BTreeMap;

use sqlx::SqlitePool;

use crate::db::models::FileEntry;
use crate::error::{Result, S3GalleryError};

/// A timeline entry containing files from a specific date.
#[derive(Debug, Clone)]
pub struct TimelineEntry {
    pub date: String,
    pub files: Vec<FileEntry>,
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
    db: &SqlitePool,
    host_id: Option<&str>,
    page: u32,
    page_size: u32,
    tag: Option<&str>,
) -> Result<(Vec<TimelineEntry>, bool)> {
    // Build the query with optional host_id and tag filters
    let files: Vec<FileEntry> = if let Some(tag_name) = tag {
        if tag_name.is_empty() {
            return Ok((Vec::new(), false));
        }
        if let Some(hid) = host_id {
            sqlx::query_as(
                "SELECT f.* FROM files f \
                 INNER JOIN file_tags ft ON f.key = ft.file_key \
                 INNER JOIN tags t ON ft.tag_id = t.tag_id \
                 WHERE t.tag_name = ? AND f.host_id = ? AND f.is_deleted = 0 \
                 ORDER BY f.effective_date DESC, f.last_modified DESC",
            )
            .bind(tag_name)
            .bind(hid)
            .fetch_all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        } else {
            sqlx::query_as(
                "SELECT f.* FROM files f \
                 INNER JOIN file_tags ft ON f.key = ft.file_key \
                 INNER JOIN tags t ON ft.tag_id = t.tag_id \
                 WHERE t.tag_name = ? AND f.is_deleted = 0 \
                 ORDER BY f.effective_date DESC, f.last_modified DESC",
            )
            .bind(tag_name)
            .fetch_all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        }
    } else {
        if let Some(hid) = host_id {
            sqlx::query_as(
                "SELECT * FROM files WHERE host_id = ? AND is_deleted = 0 \
                 ORDER BY effective_date DESC, last_modified DESC",
            )
            .bind(hid)
            .fetch_all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        } else {
            sqlx::query_as(
                "SELECT * FROM files WHERE is_deleted = 0 \
                 ORDER BY effective_date DESC, last_modified DESC",
            )
            .fetch_all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        }
    };

    // Group by date (effective_date or last_modified)
    let mut grouped: BTreeMap<String, Vec<FileEntry>> = BTreeMap::new();
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
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let (entries, has_more) = get_timeline_gallery(&pool, None, 0, 10, None).await?;
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
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let (entries, has_more) = get_timeline_gallery(&pool, None, 0, 1, None).await?;
        assert_eq!(entries.len(), 1);
        assert!(has_more);
        assert_eq!(entries[0].date, "2024-02-20");

        let (entries, has_more) = get_timeline_gallery(&pool, None, 1, 1, None).await?;
        assert_eq!(entries.len(), 1);
        assert!(!has_more);
        assert_eq!(entries[0].date, "2024-01-15");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_timeline_gallery_empty() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        let (entries, has_more) = get_timeline_gallery(&pool, None, 0, 10, None).await?;
        assert!(entries.is_empty());
        assert!(!has_more);
        Ok(())
    }
}