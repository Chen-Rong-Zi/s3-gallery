use std::sync::Arc;
use std::time::Instant;
use sqlx::SqlitePool;
use chrono::Utc;

use crate::error::{OssgalleyError, Result};
use crate::types::{BucketName, ObjectKey, FileType};
use crate::s3::client::{S3Client, ObjectSummary};
use crate::s3::lock::acquire_lock;
use crate::db::models::{FileEntry, ScanMetadata};
use crate::classify::classifier::{classify_extension, content_type_from_extension, parse_extension};
use crate::util::concurrency::ConcurrencyLimiter;
use super::diff::diff_objects;

/// Configuration for a scan operation.
pub struct ScanConfig {
    pub s3: Arc<dyn S3Client>,
    pub db: SqlitePool,
    pub bucket: BucketName,
    pub prefix: ObjectKey,
    pub concurrency: usize,
    pub extract_metadata: bool,
    pub generate_thumbnails: bool,
    pub client_id: String,
}

/// Result of a scan operation.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub total_files: u64,
    pub total_size: u64,
    pub new_files: u64,
    pub changed_files: u64,
    pub deleted_files: u64,
    pub metadata_extracted: u64,
    pub duration_secs: f64,
}

/// Run a full scan: list all objects, classify, update DB, extract metadata.
///
/// This is the main entry point for scanning an OSS bucket. It:
/// 1. Acquires a distributed lock to prevent concurrent scans
/// 2. Lists all objects from S3
/// 3. Diffs against existing DB state
/// 4. Updates DB with new/changed/deleted files
/// 5. (Optionally) Extracts metadata and generates thumbnails
/// 6. Updates scan metadata
/// 7. Releases the lock
///
/// # Errors
///
/// Returns an error if:
/// - Lock acquisition fails
/// - S3 listing fails
/// - Database operations fail
pub async fn run_scan(config: ScanConfig) -> Result<ScanResult> {
    let start = Instant::now();
    let _limiter = ConcurrencyLimiter::new(config.concurrency);

    // Step 1: Acquire lock
    let prefix_str = config.prefix.as_str().trim_end_matches('/');
    let lock_key = ObjectKey::new(format!("{prefix_str}/.ossgallery/db.lock"))
        .map_err(|e| OssgalleyError::Internal(format!("Failed to create lock key: {}", e)))?;

    let guard = acquire_lock(
        config.s3.clone(),
        config.bucket.clone(),
        lock_key,
        config.client_id.clone(),
    ).await?;

    // Step 2: List all objects from S3, filter out .ossgallery directory
    let all_objects = config.s3.list_objects(&config.bucket, &config.prefix).await?;
    let s3_objects: Vec<ObjectSummary> = all_objects
        .into_iter()
        .filter(|obj| !obj.key.as_str().contains("/.ossgallery/"))
        .collect();

    // Step 3: Get existing DB entries
    let db_entries = FileEntry::list_by_prefix(&config.db, config.prefix.as_str()).await?;

    // Step 4: Diff
    let diff = diff_objects(&s3_objects, &db_entries);

    // Step 5: Process new/changed files
    let mut new_files = 0u64;
    let mut changed_files = 0u64;
    let mut total_size = 0u64;

    // Process new objects
    for obj in &diff.new_objects {
        total_size += obj.size.as_u64();
        let file_type = classify_file(&obj.key);
        let content_type = get_content_type(&obj.key);

        FileEntry::upsert(&config.db, &FileEntry {
            key: obj.key.as_str().to_string(),
            etag: obj.etag.as_str().to_string(),
            size: obj.size.as_u64() as i64,
            last_modified: obj.last_modified.clone(),
            content_type,
            file_type: file_type.to_string(),
            metadata_state: "pending".to_string(),
            is_deleted: false,
        }).await?;

        new_files += 1;
    }

    // Process changed objects
    for obj in &diff.changed_objects {
        total_size += obj.size.as_u64();
        let file_type = classify_file(&obj.key);
        let content_type = get_content_type(&obj.key);

        FileEntry::upsert(&config.db, &FileEntry {
            key: obj.key.as_str().to_string(),
            etag: obj.etag.as_str().to_string(),
            size: obj.size.as_u64() as i64,
            last_modified: obj.last_modified.clone(),
            content_type,
            file_type: file_type.to_string(),
            metadata_state: "pending".to_string(),
            is_deleted: false,
        }).await?;

        changed_files += 1;
    }

    // Mark deleted files
    for key in &diff.deleted_keys {
        FileEntry::mark_deleted(&config.db, key).await?;
    }

    // Step 6: Extract metadata (if enabled) - placeholder for future implementation
    // In a full implementation, this would use the ExtractorRegistry to process
    // files with metadata_state = "pending"
    let metadata_extracted = 0u64;

    // Step 7: Generate thumbnails (if enabled) - placeholder for future implementation
    // In a full implementation, this would use the ThumbnailCache

    // Calculate total size including unchanged files
    for obj in &s3_objects {
        total_size += obj.size.as_u64();
    }
    // Subtract new/changed since we added them earlier (avoid double-counting)
    for obj in &diff.new_objects {
        total_size -= obj.size.as_u64();
    }
    for obj in &diff.changed_objects {
        total_size -= obj.size.as_u64();
    }

    // Step 8: Update scan_metadata
    let scan_meta = ScanMetadata {
        last_scanned_key: Some(String::new()),
        last_scanned_at: Some(Utc::now().to_rfc3339()),
        total_files: Some(s3_objects.len() as i64),
        total_size: Some(total_size as i64),
        db_schema_version: 1,
    };
    ScanMetadata::update(&config.db, &scan_meta).await?;

    // Step 9: Release lock
    guard.release().await?;

    let duration = start.elapsed();

    Ok(ScanResult {
        total_files: s3_objects.len() as u64,
        total_size,
        new_files,
        changed_files,
        deleted_files: diff.deleted_keys.len() as u64,
        metadata_extracted,
        duration_secs: duration.as_secs_f64(),
    })
}

/// Classify a file by its extension.
fn classify_file(key: &ObjectKey) -> FileType {
    if let Some(name) = key.file_name() {
        if let Some(ext) = parse_extension(name) {
            return classify_extension(&ext);
        }
    }
    FileType::Unknown
}

/// Get content type from file extension.
fn get_content_type(key: &ObjectKey) -> Option<String> {
    if let Some(name) = key.file_name() {
        if let Some(ext) = parse_extension(name) {
            return content_type_from_extension(&ext);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s3::mock::MockS3Client;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_classify_file_jpg() -> Result<()> {
        let key = ObjectKey::new("photos/test.jpg")?;
        assert_eq!(classify_file(&key), FileType::Jpeg);
        Ok(())
    }

    #[tokio::test]
    async fn test_classify_file_unknown() -> Result<()> {
        let key = ObjectKey::new("files/data.xyz")?;
        assert_eq!(classify_file(&key), FileType::Unknown);
        Ok(())
    }

    #[tokio::test]
    async fn test_classify_file_no_extension() -> Result<()> {
        let key = ObjectKey::new("files/README")?;
        assert_eq!(classify_file(&key), FileType::Unknown);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_content_type_jpg() -> Result<()> {
        let key = ObjectKey::new("photos/test.jpg")?;
        assert_eq!(get_content_type(&key), Some("image/jpeg".to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn test_get_content_type_unknown() -> Result<()> {
        let key = ObjectKey::new("files/data.xyz")?;
        assert_eq!(get_content_type(&key), None);
        Ok(())
    }

    #[tokio::test]
    async fn test_scan_empty_bucket() -> Result<()> {
        let dir = tempdir().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;

        let config = ScanConfig {
            s3: s3.clone(),
            db: pool.clone(),
            bucket: BucketName::new("test-bucket")?,
            prefix: ObjectKey::new("test")?,
            concurrency: 10,
            extract_metadata: false,
            generate_thumbnails: false,
            client_id: "test-client".to_string(),
        };

        let result = run_scan(config).await?;
        assert_eq!(result.total_files, 0);
        assert_eq!(result.new_files, 0);
        Ok(())
    }

    // Note: More comprehensive tests would require adding objects to the MockS3Client,
    // which currently only implements the S3Client trait but may not have a way to
    // add test objects. In a real test suite, we'd extend MockS3Client for that.
}
