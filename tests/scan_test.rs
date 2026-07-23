//! Integration tests for the full scan flow.
//!
//! Tests Scanner with a MockS3Client, verifying that S3 objects are correctly
//! discovered, diffed against the DB, and persisted.

use std::sync::Arc;

use s3_gallery_core::db::models::FileEntry;
use s3_gallery_core::error::Result;
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::mock::MockS3Client;
use s3_gallery_core::scan::scanner::{run_scan, ScanConfig};
use s3_gallery_core::types::{BucketName, ObjectKey};
use sqlx::SqlitePool;

mod common;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a ScanConfig for testing.
fn make_scan_config(
    s3: Arc<dyn S3Client>,
    db: SqlitePool,
    bucket: BucketName,
    prefix: &str,
) -> ScanConfig {
    // ObjectKey must be non-empty.  The test objects are placed under "test/"
    // so we use "test/" as the listing prefix.
    let listing_prefix = if prefix.is_empty() { "test/" } else { prefix };
    ScanConfig {
        host_id: "test-host".to_string(),
        s3,
        db,
        bucket,
        prefix: ObjectKey::new(listing_prefix).expect("valid prefix"),
        concurrency: 10,
        extract_metadata: false,
        generate_thumbnails: false,
        client_id: "test-client".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scan_empty_bucket() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket = common::test_bucket()?;

    let config = make_scan_config(s3, pool, bucket, "test");
    let result = run_scan(config).await?;

    assert_eq!(result.total_files, 0);
    assert_eq!(result.new_files, 0);
    assert_eq!(result.changed_files, 0);
    assert_eq!(result.deleted_files, 0);
    Ok(())
}

#[tokio::test]
async fn test_scan_discovers_new_objects() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = MockS3Client::with_fixtures(vec![
        ("test/photos/img001.jpg", b"jpeg data"),
        ("test/photos/img002.jpg", b"jpeg data"),
        ("test/docs/readme.txt", b"text data"),
    ])?;
    let bucket = common::test_bucket()?;

    let config = make_scan_config(Arc::new(s3), pool.clone(), bucket, "");
    let result = run_scan(config).await?;

    assert_eq!(result.total_files, 3, "should discover 3 objects");
    assert_eq!(result.new_files, 3, "all 3 should be new");

    // Verify DB entries were created.
    let count = FileEntry::count(&pool, "test-host").await?;
    assert_eq!(count, 3);

    // Verify specific entries.
    let entry = FileEntry::get_by_key(&pool, "test-host", "test/photos/img001.jpg").await?;
    assert_eq!(entry.file_type, "jpeg");
    assert!(!entry.is_deleted);

    let entry = FileEntry::get_by_key(&pool, "test-host", "test/docs/readme.txt").await?;
    // "txt" is not a recognized file type, so it's classified as "unknown".
    assert_eq!(entry.file_type, "unknown");
    Ok(())
}

#[tokio::test]
async fn test_scan_detects_modified_objects() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let bucket = common::test_bucket()?;

    // Insert a file entry with an old etag.
    let file = FileEntry {
        host_id: "test-host".to_string(),
        key: "test/photos/img001.jpg".to_string(),
        etag: "old-etag".to_string(),
        size: 100,
        last_modified: "2026-01-01T00:00:00Z".to_string(),
        content_type: Some("image/jpeg".to_string()),
        file_type: "jpeg".to_string(),
        metadata_state: "pending".to_string(),
        effective_date: "".to_string(),
        is_deleted: false,
    };
    FileEntry::insert(&pool, &file).await?;

    // The mock S3 generates a random etag on insert, so the object will have
    // a different etag than what's in the DB, triggering a "changed" detection.
    let s3 = MockS3Client::with_fixtures(vec![("test/photos/img001.jpg", b"updated jpeg data")])?;
    let config = make_scan_config(Arc::new(s3), pool.clone(), bucket, "");
    let result = run_scan(config).await?;

    assert_eq!(result.total_files, 1);
    assert_eq!(result.new_files, 0);
    assert_eq!(
        result.changed_files, 1,
        "etag differs, should be detected as changed"
    );
    assert_eq!(result.deleted_files, 0);

    // Verify the etag was updated in the DB.
    let updated = FileEntry::get_by_key(&pool, "test-host", "test/photos/img001.jpg").await?;
    assert_ne!(updated.etag, "old-etag", "etag should have been updated");
    assert_eq!(
        updated.size, 17,
        "size should match 'updated jpeg data' len"
    );
    Ok(())
}

#[tokio::test]
async fn test_scan_detects_deleted_objects() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let bucket = common::test_bucket()?;

    // Insert a file that exists in the DB but not in S3.
    let file = FileEntry {
        host_id: "test-host".to_string(),
        key: "test/photos/ghost.txt".to_string(),
        etag: "ghost-etag".to_string(),
        size: 50,
        last_modified: "2026-01-01T00:00:00Z".to_string(),
        content_type: None,
        file_type: "txt".to_string(),
        metadata_state: "pending".to_string(),
        effective_date: "".to_string(),
        is_deleted: false,
    };
    FileEntry::insert(&pool, &file).await?;

    let s3 = MockS3Client::new(); // empty — no objects
    let config = make_scan_config(Arc::new(s3), pool.clone(), bucket, "");
    let result = run_scan(config).await?;

    assert_eq!(result.total_files, 0);
    assert_eq!(result.new_files, 0);
    assert_eq!(result.changed_files, 0);
    assert_eq!(
        result.deleted_files, 1,
        "ghost.txt should be marked deleted"
    );

    // Verify the file is soft-deleted.
    let entry = FileEntry::get_by_key(&pool, "test-host", "test/photos/ghost.txt").await?;
    assert!(entry.is_deleted);
    Ok(())
}

#[tokio::test]
async fn test_scan_mixed_new_changed_deleted() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let bucket = common::test_bucket()?;

    // Pre-populate DB with one unchanged, one that will be "changed" (different
    // etag), and one that will be "deleted" (not in S3).
    let db_files = vec![
        FileEntry {
            host_id: "test-host".to_string(),
            key: "test/unchanged.txt".to_string(),
            etag: "u-etag".to_string(),
            size: 10,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: None,
            file_type: "txt".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
        FileEntry {
            host_id: "test-host".to_string(),
            key: "test/changed.txt".to_string(),
            etag: "old-etag".to_string(),
            size: 20,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: None,
            file_type: "txt".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
        FileEntry {
            host_id: "test-host".to_string(),
            key: "test/deleted.txt".to_string(),
            etag: "d-etag".to_string(),
            size: 30,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: None,
            file_type: "txt".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
    ];
    for f in &db_files {
        FileEntry::insert(&pool, f).await?;
    }

    // S3 has: unchanged.txt, changed.txt, and new.txt (not in DB).
    // Note: MockS3Client generates random etags, so the etag for unchanged.txt
    // will differ from "u-etag", making it "changed" too.
    let s3 = MockS3Client::with_fixtures(vec![
        ("test/unchanged.txt", b"data"),
        ("test/changed.txt", b"data"),
        ("test/new.txt", b"new data"),
    ])?;
    let config = make_scan_config(Arc::new(s3), pool.clone(), bucket, "");
    let result = run_scan(config).await?;

    // At minimum we should see:
    assert_eq!(result.total_files, 3, "3 objects in S3");
    assert_eq!(result.new_files, 1, "new.txt is new");
    assert_eq!(result.deleted_files, 1, "deleted.txt was removed");

    // Verify new.txt was inserted.
    let new_entry = FileEntry::get_by_key(&pool, "test-host", "test/new.txt").await?;
    assert!(!new_entry.is_deleted);

    // Verify deleted.txt was soft-deleted.
    let deleted_entry = FileEntry::get_by_key(&pool, "test-host", "test/deleted.txt").await?;
    assert!(deleted_entry.is_deleted);

    Ok(())
}

#[tokio::test]
async fn test_scan_updates_scan_metadata() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = MockS3Client::with_fixtures(vec![("test/a.jpg", b"data"), ("test/b.jpg", b"data")])?;

    let config = make_scan_config(Arc::new(s3), pool.clone(), common::test_bucket()?, "");
    let result = run_scan(config).await?;

    assert_eq!(result.total_files, 2);
    assert_eq!(result.total_size, 8); // 2 * b"data".len()

    // Check scan_metadata was updated.
    use s3_gallery_core::db::models::ScanMetadata;
    let meta = ScanMetadata::get(&pool, "test-host").await?;
    assert!(meta.last_scanned_at.is_some());
    assert_eq!(meta.total_files, Some(2));
    assert_eq!(meta.total_size, Some(8));
    Ok(())
}

#[tokio::test]
async fn test_scan_skips_s3_gallery_directory() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = MockS3Client::with_fixtures(vec![
        ("test/photos/img.jpg", b"data"),
        ("test/metadata/.s3-gallery/host.config.json", b"config data"),
    ])?;

    let config = make_scan_config(Arc::new(s3), pool.clone(), common::test_bucket()?, "");
    let result = run_scan(config).await?;

    // Only the non-.s3-gallery file should be counted.
    assert_eq!(
        result.total_files, 1,
        "s3-gallery directory should be filtered out"
    );
    assert_eq!(result.new_files, 1);

    // Verify the photo file was recorded.
    let entry = FileEntry::get_by_key(&pool, "test-host", "test/photos/img.jpg").await?;
    assert!(!entry.is_deleted);
    Ok(())
}
