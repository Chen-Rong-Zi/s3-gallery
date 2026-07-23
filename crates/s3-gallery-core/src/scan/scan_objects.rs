//! ScanObjectEntry — snapshot of S3 objects for one scan.

use sqlx::SqlitePool;

use crate::error::{Result, S3GalleryError};
use crate::s3::client::ObjectSummary;

/// A row in the `scan_objects` table — snapshot of S3 listing for one scan.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScanObjectEntry {
    pub scan_id: String,
    pub host_id: String,
    pub key: String,
    pub etag: String,
    pub size: i64,
    pub last_modified: String,
    pub is_deleted: bool,
}

impl ScanObjectEntry {
    /// Batch insert scan objects from an S3 listing.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn batch_insert(
        pool: &SqlitePool,
        scan_id: &str,
        host_id: &str,
        objects: &[ObjectSummary],
    ) -> Result<()> {
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to begin transaction: {e}")))?;

        for obj in objects {
            sqlx::query(
                "INSERT OR IGNORE INTO scan_objects (scan_id, host_id, key, etag, size, last_modified, is_deleted) \
                 VALUES (?, ?, ?, ?, ?, ?, 0)",
            )
            .bind(scan_id)
            .bind(host_id)
            .bind(obj.key.as_str())
            .bind(obj.etag.as_str())
            .bind(obj.size.as_u64() as i64)
            .bind(&obj.last_modified)
            .execute(&mut *tx)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to insert scan object: {e}")))?;
        }

        tx.commit()
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to commit transaction: {e}")))?;

        Ok(())
    }

    /// List all scan objects for a given scan_id and host_id.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn list_by_scan(
        pool: &SqlitePool,
        scan_id: &str,
        host_id: &str,
    ) -> Result<Vec<ScanObjectEntry>> {
        sqlx::query_as::<_, ScanObjectEntry>(
            "SELECT * FROM scan_objects WHERE scan_id = ? AND host_id = ? AND is_deleted = 0 ORDER BY key",
        )
        .bind(scan_id)
        .bind(host_id)
        .fetch_all(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to list scan objects: {e}")))
    }

    /// Delete all scan objects for a given scan_id.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn delete_by_scan(pool: &SqlitePool, scan_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM scan_objects WHERE scan_id = ?")
            .bind(scan_id)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to delete scan objects: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::s3::client::ObjectSummary;
    use crate::types::{Etag, FileSize, ObjectKey};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_batch_insert_and_list() -> Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let objects = vec![
            ObjectSummary {
                key: ObjectKey::new("photos/a.jpg")?,
                etag: Etag::new("e1")?,
                size: FileSize::new(100),
                last_modified: "2026-01-01T00:00:00Z".to_string(),
            },
            ObjectSummary {
                key: ObjectKey::new("photos/b.jpg")?,
                etag: Etag::new("e2")?,
                size: FileSize::new(200),
                last_modified: "2026-01-01T00:00:00Z".to_string(),
            },
        ];

        ScanObjectEntry::batch_insert(&pool, "scan-1", "host-1", &objects).await?;

        let entries = ScanObjectEntry::list_by_scan(&pool, "scan-1", "host-1").await?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "photos/a.jpg");
        assert_eq!(entries[1].key, "photos/b.jpg");

        // Delete by scan
        ScanObjectEntry::delete_by_scan(&pool, "scan-1").await?;
        let entries = ScanObjectEntry::list_by_scan(&pool, "scan-1", "host-1").await?;
        assert!(entries.is_empty());

        Ok(())
    }
}