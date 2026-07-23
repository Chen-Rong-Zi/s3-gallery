use chrono::Utc;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Instant;

use super::diff::diff_objects;
use crate::classify::classifier::{
    classify_extension, content_type_from_extension, parse_extension,
};
use crate::db::models::{FileEntry, ScanMetadata};
use crate::error::{Result, S3GalleryError};
use crate::extractor::exif::ExifExtractor;
use crate::extractor::registry::ExtractorRegistry;
use crate::extractor::tag_rules::TagRule;
use crate::s3::client::{ObjectSummary, S3Client};
use crate::s3::lock::acquire_lock;
use crate::types::{BucketName, FileType, ObjectKey};
use crate::util::concurrency::ConcurrencyLimiter;

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
    pub host_id: String,
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

/// Core scan logic — pure business logic, no lock acquisition.
///
/// # Errors
///
/// Returns an error if any S3 or database operation fails.
async fn scan_core(config: ScanConfig) -> Result<ScanResult> {
    let start = Instant::now();
    let _limiter = ConcurrencyLimiter::new(config.concurrency);

    // Step 2: List all objects from S3, filter out .s3-gallery directory
    let all_objects = config
        .s3
        .list_objects(&config.bucket, &config.prefix)
        .await?;
    let s3_objects: Vec<ObjectSummary> = all_objects
        .into_iter()
        .filter(|obj| {
            let key = obj.key.as_str();
            !key.contains("/.s3-gallery/") && !key.starts_with(".s3-gallery/")
        })
        .collect();

    tracing::info!(
        target: "s3_gallery::scan",
        total_objects = s3_objects.len(),
        "Objects listed from S3"
    );

    // Step 3: Get existing DB entries
    let db_entries =
        FileEntry::list_by_prefix(&config.db, config.host_id.as_str(), config.prefix.as_str())
            .await?;

    // Step 4: Diff
    let diff = diff_objects(&s3_objects, &db_entries);

    tracing::debug!(
        target: "s3_gallery::scan",
        new = diff.new_objects.len(),
        changed = diff.changed_objects.len(),
        deleted = diff.deleted_keys.len(),
        "Diff completed"
    );

    // Step 5: Process new/changed files
    let mut new_files = 0u64;
    let mut changed_files = 0u64;
    let mut total_size = 0u64;

    // Process new objects
    for obj in &diff.new_objects {
        total_size += obj.size.as_u64();
        let file_type = classify_file(&obj.key);
        let content_type = get_content_type(&obj.key);

        FileEntry::upsert(
            &config.db,
            &FileEntry {
                host_id: config.host_id.clone(),
                key: obj.key.as_str().to_string(),
                etag: obj.etag.as_str().to_string(),
                size: obj.size.as_u64() as i64,
                last_modified: obj.last_modified.clone(),
                content_type,
                file_type: file_type.to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        new_files += 1;
    }

    // Process changed objects
    for obj in &diff.changed_objects {
        total_size += obj.size.as_u64();
        let file_type = classify_file(&obj.key);
        let content_type = get_content_type(&obj.key);

        FileEntry::upsert(
            &config.db,
            &FileEntry {
                host_id: config.host_id.clone(),
                key: obj.key.as_str().to_string(),
                etag: obj.etag.as_str().to_string(),
                size: obj.size.as_u64() as i64,
                last_modified: obj.last_modified.clone(),
                content_type,
                file_type: file_type.to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        changed_files += 1;
    }

    // Mark deleted files
    for key in &diff.deleted_keys {
        FileEntry::mark_deleted(&config.db, config.host_id.as_str(), key).await?;
    }

    // Step 6: Extract metadata (if enabled)
    let mut metadata_extracted = 0u64;
    if config.extract_metadata {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(ExifExtractor::new()));
        let tag_rules = TagRule::default_rules();

        for obj in diff.new_objects.iter().chain(diff.changed_objects.iter()) {
            match process_file_metadata(&config, &registry, &tag_rules, obj).await {
                Ok(count) => metadata_extracted += count,
                Err(e) => {
                    tracing::warn!(key = %obj.key.as_str(), error = %e, "metadata processing failed");
                }
            }
        }
    }

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
        host_id: config.host_id.clone(),
        last_scanned_key: Some(String::new()),
        last_scanned_at: Some(Utc::now().to_rfc3339()),
        total_files: Some(s3_objects.len() as i64),
        total_size: Some(total_size as i64),
        db_schema_version: 1,
    };
    ScanMetadata::update(&config.db, &scan_meta).await?;

    // Step 8.5: Compute and store directory sizes
    {
        use std::collections::HashMap;
        let mut dir_agg: HashMap<String, (u64, u64)> = HashMap::new();

        for obj in &s3_objects {
            let key = obj.key.as_str();
            // Extract all directory prefixes from the key
            // e.g., "photos/2023/autumn/img.jpg" -> "photos/", "photos/2023/", "photos/2023/autumn/"
            let mut pos = 0;
            while let Some(slash) = key[pos..].find('/') {
                let prefix_end = pos + slash + 1; // include trailing '/'
                let dir_path = &key[..prefix_end];
                let entry = dir_agg.entry(dir_path.to_string()).or_insert((0, 0));
                entry.0 += obj.size.as_u64();
                entry.1 += 1;
                pos = prefix_end;
            }
        }

        // Batch upsert in a transaction
        let mut tx =
            config.db.begin().await.map_err(|e| {
                S3GalleryError::DbError(format!("Failed to begin transaction: {e}"))
            })?;
        for (dir_path, (total_size, total_files)) in &dir_agg {
            sqlx::query(
                "INSERT OR REPLACE INTO dir_sizes (host_id, dir_path, total_size, total_files) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(&config.host_id)
            .bind(dir_path)
            .bind(*total_size as i64)
            .bind(*total_files as i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to upsert dir size: {e}")))?;
        }
        tx.commit()
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to commit transaction: {e}")))?;

        tracing::debug!(
            target: "s3_gallery::scan",
            host_id = %config.host_id,
            directories = dir_agg.len(),
            "Directory sizes computed"
        );
    }

    let duration = start.elapsed();

    tracing::info!(
        target: "s3_gallery::scan",
        total_files = s3_objects.len(),
        new_files = new_files,
        changed_files = changed_files,
        deleted_files = diff.deleted_keys.len(),
        duration_secs = duration.as_secs_f64(),
        "Scan completed"
    );

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
    tracing::info!(
        target: "s3_gallery::scan",
        prefix = %config.prefix,
        client_id = %config.client_id,
        "Scan started"
    );

    // Step 1: Acquire lock
    // Lock key is at bucket root
    let lock_key = ObjectKey::new("s3-gallery.lock".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Failed to create lock key: {}", e)))?;

    let guard = acquire_lock(
        config.s3.clone(),
        config.bucket.clone(),
        lock_key,
        config.client_id.clone(),
    )
    .await?;

    // Steps 2-8: Core business logic
    let result = scan_core(config).await;

    // Step 9: Release lock
    guard.release().await?;

    result
}

/// Process metadata for a single object: download, extract, store, tag.
async fn process_file_metadata(
    config: &ScanConfig,
    registry: &ExtractorRegistry,
    tag_rules: &[TagRule],
    obj: &ObjectSummary,
) -> Result<u64> {
    use crate::db::models::{FileTagEntry, MetadataEntry, TagEntry};
    use crate::extractor::tag_rules::evaluate_all;
    use crate::extractor::tag_rules::parse_dms;

    let key = obj.key.as_str();
    let file_name = key.rsplit('/').next().unwrap_or(key);
    let ext = match parse_extension(file_name) {
        Some(e) => e,
        None => return Ok(0),
    };
    let file_type = classify_extension(&ext).to_string();

    // Check if any extractor supports this file type
    if registry.find(&file_type, ext.as_str()).is_empty() {
        // Mark as extracted so we don't retry on every scan
        let _result = sqlx::query(
            "UPDATE files SET metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
        )
        .bind(&config.host_id)
        .bind(key)
        .execute(&config.db)
        .await;
        return Ok(0);
    }

    // Download only the first 64KB (EXIF data is in the APP1 marker,
    // which is always near the start of the JPEG file)
    let data = match config
        .s3
        .get_object_range(&config.bucket, &obj.key, 0, 65536)
        .await
    {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(key = %key, error = %e, "failed to download range for metadata extraction");
            return Ok(0);
        }
    };

    // Extract metadata
    let items = match registry.extract_all(&data, &file_type, ext.as_str()).await {
        Ok(items) => items,
        Err(e) => {
            tracing::warn!(key = %key, error = %e, "metadata extraction failed");
            // Mark as failed so we don't retry on every scan
            let _result = sqlx::query(
                "UPDATE files SET metadata_state = 'failed' WHERE host_id = ? AND key = ?",
            )
            .bind(&config.host_id)
            .bind(key)
            .execute(&config.db)
            .await;
            return Ok(0);
        }
    };

    if items.is_empty() {
        // Mark as extracted so we don't retry on every scan
        let _result = sqlx::query(
            "UPDATE files SET metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
        )
        .bind(&config.host_id)
        .bind(key)
        .execute(&config.db)
        .await;
        return Ok(0);
    }

    // Store metadata
    let now = Utc::now().to_rfc3339();
    for item in &items {
        MetadataEntry::insert(
            &config.db,
            &MetadataEntry {
                file_key: key.to_string(),
                namespace: item.namespace.to_string(),
                key: item.key.clone(),
                value: item.value.clone(),
                extracted_at: now.clone(),
                partial: false,
            },
        )
        .await?;
    }

    // Generate and store tags
    let tags = evaluate_all(tag_rules, &items, &file_type);
    for tag in &tags {
        let tag_id = TagEntry::ensure_exists(&config.db, &tag.tag_name, &tag.tag_type).await?;
        FileTagEntry::insert(
            &config.db,
            &FileTagEntry {
                file_key: key.to_string(),
                tag_id,
            },
        )
        .await?;
    }

    // Add exif:yes tag for all files with extracted metadata
    let exif_tag_id = TagEntry::ensure_exists(&config.db, "exif:yes", "auto").await?;
    FileTagEntry::insert(
        &config.db,
        &FileTagEntry {
            file_key: key.to_string(),
            tag_id: exif_tag_id,
        },
    )
    .await?;

    // Find GPS coordinates in extracted metadata and reverse geocode
    let gps_lat = items
        .iter()
        .find(|m| m.key == "GPSLatitude")
        .map(|m| parse_dms(&m.value));
    let gps_lon = items
        .iter()
        .find(|m| m.key == "GPSLongitude")
        .map(|m| parse_dms(&m.value));

    if let (Some(lat), Some(lon)) = (gps_lat, gps_lon) {
        let geocoder = crate::extractor::geocode::Geocoder::from_embedded();
        if let Some(location) = geocoder.reverse_geocode(lat, lon) {
            // Add location:city tag
            let city_tag =
                TagEntry::ensure_exists(&config.db, &format!("location:{}", location.city), "auto")
                    .await?;
            FileTagEntry::insert(
                &config.db,
                &FileTagEntry {
                    file_key: key.to_string(),
                    tag_id: city_tag,
                },
            )
            .await?;

            // Add location:district tag (more precise)
            let district_tag = TagEntry::ensure_exists(
                &config.db,
                &format!("location:{}", location.district),
                "auto",
            )
            .await?;
            FileTagEntry::insert(
                &config.db,
                &FileTagEntry {
                    file_key: key.to_string(),
                    tag_id: district_tag,
                },
            )
            .await?;
        }
    }

    // Update effective_date (EXIF DateTimeOriginal > last_modified)
    // EXIF format is "2024:07:22 10:30:00" (colons), convert to "2024-07-22"
    let exif_date = items
        .iter()
        .find(|m| m.key == "DateTimeOriginal" || m.key == "DateTimeDigitized")
        .map(|m| m.value.as_str())
        .and_then(|v| v.get(..10))
        .map(|d| d.replace(":", "-"));
    let effective_date = exif_date
        .as_deref()
        .unwrap_or_else(|| &obj.last_modified[..10.min(obj.last_modified.len())]);
    sqlx::query("UPDATE files SET effective_date = ?, metadata_state = 'extracted' WHERE host_id = ? AND key = ?")
        .bind(effective_date)
        .bind(&config.host_id)
        .bind(key)
        .execute(&config.db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    Ok(1)
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
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::s3::mock::MockS3Client;
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
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
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
            host_id: "test-host".to_string(),
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
