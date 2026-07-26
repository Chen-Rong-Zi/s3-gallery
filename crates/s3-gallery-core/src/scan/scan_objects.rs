//! ScanObjectEntry — snapshot of S3 objects for one scan.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Statement};

use crate::entity::scan_object;
use crate::error::{Result, S3GalleryError};
use crate::s3::client::ObjectSummary;

/// A row in the `scan_objects` table — snapshot of S3 listing for one scan.
#[derive(Debug, Clone)]
pub struct ScanObjectEntry {
    pub scan_id: String,
    pub host_id: String,
    pub key: String,
    pub etag: String,
    pub size: i64,
    pub last_modified: String,
    pub is_deleted: bool,
}

impl From<scan_object::Model> for ScanObjectEntry {
    fn from(m: scan_object::Model) -> Self {
        Self {
            scan_id: m.scan_id,
            host_id: m.host_id.to_string(),
            key: m.key.to_string(),
            etag: m.etag.to_string(),
            size: m.size.as_u64() as i64,
            last_modified: m.last_modified,
            is_deleted: m.is_deleted,
        }
    }
}

impl ScanObjectEntry {
    /// Batch insert scan objects from an S3 listing.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn batch_insert(
        db: &DatabaseConnection,
        scan_id: &str,
        host_id: &str,
        objects: &[ObjectSummary],
    ) -> Result<()> {
        use sea_orm::ConnectionTrait;

        let mut batch_params: Vec<sea_orm::Value> = Vec::new();
        let mut placeholders: Vec<String> = Vec::new();

        for obj in objects {
            placeholders.push("(?, ?, ?, ?, ?, ?, 0)".to_string());
            batch_params.push(sea_orm::Value::String(Some(Box::new(scan_id.to_string()))));
            batch_params.push(sea_orm::Value::String(Some(Box::new(host_id.to_string()))));
            batch_params.push(sea_orm::Value::String(Some(Box::new(
                obj.key.as_str().to_string(),
            ))));
            batch_params.push(sea_orm::Value::String(Some(Box::new(
                obj.etag.as_str().to_string(),
            ))));
            batch_params.push(sea_orm::Value::BigInt(Some(obj.size.as_u64() as i64)));
            batch_params.push(sea_orm::Value::String(Some(Box::new(
                obj.last_modified.clone(),
            ))));
        }

        if placeholders.is_empty() {
            return Ok(());
        }

        let sql = format!(
            "INSERT OR IGNORE INTO scan_objects (scan_id, host_id, key, etag, size, last_modified, is_deleted) VALUES {}",
            placeholders.join(", ")
        );

        db.execute(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &sql,
            batch_params,
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to insert scan objects: {e}")))?;

        Ok(())
    }

    /// List all scan objects for a given scan_id and host_id.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn list_by_scan(
        db: &DatabaseConnection,
        scan_id: &str,
        host_id: &str,
    ) -> Result<Vec<ScanObjectEntry>> {
        let models = scan_object::Entity::find()
            .filter(scan_object::Column::ScanId.eq(scan_id))
            .filter(scan_object::Column::HostId.eq(host_id))
            .filter(scan_object::Column::IsDeleted.eq(false))
            .order_by_asc(scan_object::Column::Key)
            .all(db)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to list scan objects: {e}")))?;

        Ok(models.into_iter().map(ScanObjectEntry::from).collect())
    }

    /// Delete all scan objects for a given scan_id.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn delete_by_scan(db: &DatabaseConnection, scan_id: &str) -> Result<()> {
        scan_object::Entity::delete_many()
            .filter(scan_object::Column::ScanId.eq(scan_id))
            .exec(db)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to delete scan objects: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate::run_full_migration;
    use crate::s3::client::ObjectSummary;
    use crate::types::{Etag, FileSize, ObjectKey};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_batch_insert_and_list() -> Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = sqlx::SqlitePool::connect(&db_url)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db = sea_orm::Database::connect(&db_url)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        run_full_migration(&db).await?;

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

        ScanObjectEntry::batch_insert(&db, "scan-1", "host-1", &objects).await?;

        let entries = ScanObjectEntry::list_by_scan(&db, "scan-1", "host-1").await?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "photos/a.jpg");
        assert_eq!(entries[1].key, "photos/b.jpg");

        // Delete by scan
        ScanObjectEntry::delete_by_scan(&db, "scan-1").await?;
        let entries = ScanObjectEntry::list_by_scan(&db, "scan-1", "host-1").await?;
        assert!(entries.is_empty());

        Ok(())
    }
}
