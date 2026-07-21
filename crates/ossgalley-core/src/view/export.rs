//! Export file list functionality.

use sqlx::SqlitePool;

use crate::db::models::FileEntry;
use crate::error::Result;

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
pub async fn export_files(db: &SqlitePool, format: ExportFormat) -> Result<String> {
    let files: Vec<FileEntry> = sqlx::query_as(
        "SELECT * FROM files WHERE is_deleted = 0 ORDER BY key"
    )
    .fetch_all(db)
    .await
    .map_err(|e| crate::error::OssgalleyError::DbError(e.to_string()))?;

    match format {
        ExportFormat::Csv => export_csv(&files),
        ExportFormat::Json => export_json(&files),
    }
}

fn export_csv(files: &[FileEntry]) -> Result<String> {
    let mut csv = String::new();
    csv.push_str("key,etag,size,last_modified,file_type\n");

    for file in files {
        let key = escape_csv(&file.key);
        let etag = escape_csv(&file.etag);
        let last_modified = escape_csv(&file.last_modified);
        let file_type = escape_csv(&file.file_type);
        csv.push_str(&format!(
            "{key},{etag},{size},{last_modified},{file_type}\n",
            size = file.size
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

fn export_json(files: &[FileEntry]) -> Result<String> {
    #[derive(serde::Serialize)]
    struct FileRow<'a> {
        key: &'a str,
        etag: &'a str,
        size: i64,
        last_modified: &'a str,
        content_type: &'a Option<String>,
        file_type: &'a str,
        metadata_state: &'a str,
    }

    let mut rows = Vec::with_capacity(files.len());
    for file in files {
        rows.push(FileRow {
            key: &file.key,
            etag: &file.etag,
            size: file.size,
            last_modified: &file.last_modified,
            content_type: &file.content_type,
            file_type: &file.file_type,
            metadata_state: &file.metadata_state,
        });
    }

    serde_json::to_string_pretty(&rows)
        .map_err(|e| crate::error::OssgalleyError::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::FileEntry;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::OssgalleyError;
    use tempfile::tempdir;

    async fn setup_test_db() -> Result<(SqlitePool, tempfile::TempDir)> {
        let dir = tempdir().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;
        Ok((pool, dir))
    }

    async fn seed_test_files(pool: &SqlitePool) -> Result<()> {
        FileEntry::upsert(
            pool,
            &FileEntry {
                key: "photo001.jpg".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 1024,
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "pending".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                key: "video.mp4".to_string(),
                etag: "\"def456\"".to_string(),
                size: 50000,
                last_modified: "2024-02-01T00:00:00Z".to_string(),
                content_type: Some("video/mp4".to_string()),
                file_type: "mp4".to_string(),
                metadata_state: "pending".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_export_csv() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let csv = export_files(&pool, ExportFormat::Csv).await?;
        assert!(csv.contains("photo001.jpg"));
        assert!(csv.contains("video.mp4"));

        Ok(())
    }

    #[tokio::test]
    async fn test_export_json() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let json = export_files(&pool, ExportFormat::Json).await?;
        assert!(json.contains("photo001.jpg"));
        assert!(json.contains("video.mp4"));

        Ok(())
    }
}
