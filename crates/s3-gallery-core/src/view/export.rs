//! Export file list functionality.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, Order, QueryFilter, QueryOrder};

use crate::entity::file;
use crate::error::Result;
use crate::error::S3GalleryError;

/// Export format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// CSV format.
    Csv,
    /// JSON format.
    Json,
}

/// Export file list.
///
/// # Errors
///
/// Returns an error if the database query fails or serialization fails.
pub async fn export_files(
    db: &DatabaseConnection,
    host_id: &str,
    format: ExportFormat,
) -> Result<String> {
    let files = file::Entity::find()
        .filter(file::Column::HostId.eq(host_id))
        .filter(file::Column::IsDeleted.eq(false))
        .order_by(file::Column::Key, Order::Asc)
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    match format {
        ExportFormat::Csv => export_csv(&files),
        ExportFormat::Json => export_json(&files),
    }
}

fn export_csv(files: &[file::Model]) -> Result<String> {
    let mut csv = String::new();
    csv.push_str("key,etag,size,last_modified,file_type\n");

    for file in files {
        let key = escape_csv(&file.key.to_string());
        let etag = escape_csv(&file.etag.to_string());
        let last_modified = escape_csv(&file.last_modified);
        let file_type = escape_csv(&file.file_type.to_string());
        csv.push_str(&format!(
            "{key},{etag},{size},{last_modified},{file_type}\n",
            size = file.size.as_u64()
        ));
    }

    Ok(csv)
}

fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

fn export_json(files: &[file::Model]) -> Result<String> {
    #[derive(serde::Serialize)]
    struct FileRow {
        key: String,
        etag: String,
        size: u64,
        last_modified: String,
        content_type: Option<String>,
        file_type: String,
        metadata_state: String,
    }

    let mut rows = Vec::with_capacity(files.len());
    for file in files {
        rows.push(FileRow {
            key: file.key.to_string(),
            etag: file.etag.to_string(),
            size: file.size.as_u64(),
            last_modified: file.last_modified.clone(),
            content_type: file.content_type.clone(),
            file_type: file.file_type.to_string(),
            metadata_state: file.metadata_state.to_string(),
        });
    }

    serde_json::to_string_pretty(&rows)
        .map_err(|e| crate::error::S3GalleryError::Internal(e.to_string()))
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

        Ok(())
    }

    #[tokio::test]
    async fn test_export_csv() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let csv = export_files(&db, "test-host", ExportFormat::Csv).await?;
        assert!(csv.contains("photo001.jpg"));
        assert!(csv.contains("video.mp4"));

        Ok(())
    }

    #[tokio::test]
    async fn test_export_json() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let json = export_files(&db, "test-host", ExportFormat::Json).await?;
        assert!(json.contains("photo001.jpg"));
        assert!(json.contains("video.mp4"));

        Ok(())
    }
}
