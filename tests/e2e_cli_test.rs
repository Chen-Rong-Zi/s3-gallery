//! End-to-end tests for CLI command flows (scan, view, db).
//!
//! All tests are marked `#[ignore]` and run against a real MinIO instance.
//! Configure via environment variables:
//!
//! - `S3_ENDPOINT` (default: `http://localhost:9000`)
//! - `AWS_ACCESS_KEY_ID` (default: `minioadmin`)
//! - `AWS_SECRET_ACCESS_KEY` (default: `minioadmin`)
//! - `S3_BUCKET` (default: `s3-gallery-e2e-test`)
//! - `S3_REGION` (default: `us-east-1`)

use std::sync::Arc;

use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::scan::scanner::{run_scan, ScanConfig};
use s3_gallery_core::types::{BucketName, ObjectKey, Prefix, SortField, SortOrder};
use s3_gallery_core::view::LocalView;
use sea_orm::DatabaseConnection;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

async fn setup_e2e_db() -> Result<(DatabaseConnection, TempDir)> {
    let dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let db_path = dir.path().join("e2e-cli-test.db");
    let db = create_pool(&db_path).await?;
    run_migrations(db.get_sqlite_connection_pool()).await?;
    Ok((db, dir))
}

async fn ensure_bucket(client: &aws_sdk_s3::Client, bucket_name: &str) -> Result<()> {
    let result = client.head_bucket().bucket(bucket_name).send().await;
    if result.is_ok() {
        return Ok(());
    }
    let create_result = client.create_bucket().bucket(bucket_name).send().await;
    match create_result {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("BucketAlreadyExists") || msg.contains("BucketAlreadyOwnedByYou") {
                return Ok(());
            }
            Err(S3GalleryError::S3Error(format!(
                "Failed to create bucket: {msg}"
            )))
        }
    }
}

/// Create S3 client, bucket name, and the underlying AWS SDK client.
///
/// Returns `(trait-object S3Client, BucketName, raw AWS SDK Client)` so
/// callers can pass the raw client to `ensure_bucket` without needing
/// `as_any()` downcasting (the `S3Client` trait does not provide it).
fn make_s3_client() -> Result<(Arc<dyn S3Client>, BucketName, aws_sdk_s3::Client)> {
    let bucket = BucketName::new(&env_or("S3_BUCKET", "s3-gallery-e2e-test"))?;
    let config = OssConfig::validate(
        bucket.clone(),
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_REGION", "us-east-1"),
        &env_or("AWS_ACCESS_KEY_ID", "minioadmin"),
        &env_or("AWS_SECRET_ACCESS_KEY", "minioadmin"),
        10,
    )?;
    let real = RealS3Client::from_config(&config);
    let aws_client = real.client.clone();
    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    Ok((s3, bucket, aws_client))
}

/// Upload test files to a unique prefix, scan them.
async fn setup_scan_fixture(
    s3: &Arc<dyn S3Client>,
    bucket: &BucketName,
    db: &DatabaseConnection,
    prefix: &str,
    host_id: &str,
) -> Result<()> {
    let test_files = vec![
        (
            format!("{prefix}/photos/2024/vacation.jpg"),
            vec![
                0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x02, 0x03,
                0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
            ],
        ),
        (
            format!("{prefix}/photos/2024/party.mp4"),
            vec![
                0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x6D, 0x70, 0x34, 0x32, 0x00, 0x00,
                0x00, 0x00, 0x6D, 0x70, 0x34, 0x32,
            ],
        ),
        (
            format!("{prefix}/photos/2023/old-photo.jpg"),
            vec![
                0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x02, 0x03,
                0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
            ],
        ),
        (
            format!("{prefix}/docs/readme.txt"),
            b"Hello, this is a readme file.".to_vec(),
        ),
    ];

    for (key, data) in &test_files {
        let object_key = ObjectKey::new(key)?;
        s3.put_object(bucket, &object_key, data).await?;
    }

    let sqlite_pool = db.get_sqlite_connection_pool().clone();
    let scan_config = ScanConfig {
        host_id: host_id.to_string(),
        s3: S3Service::new(s3.clone()),
        db: sqlite_pool,
        sea_db: db.clone(),
        bucket: bucket.clone(),
        prefix: Prefix::new(prefix)?,
        concurrency: 4,
        extract_metadata: false,
        generate_thumbnails: false,
        client_id: "e2e-cli-test".to_string(),
    };

    let result = run_scan(scan_config, String::new()).await?;
    assert_eq!(result.total_files, 4, "should find all 4 test files");
    assert_eq!(result.new_files, 4);

    Ok(())
}

async fn cleanup_prefix(s3: &Arc<dyn S3Client>, bucket: &BucketName, prefix: &str) -> Result<()> {
    let prefix_key = Prefix::new(prefix)?;
    let objects = s3.list_objects(bucket, &prefix_key).await?;
    for obj in &objects {
        s3.delete_object(bucket, &obj.key).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Task 1: view tree, ls, stat
// ---------------------------------------------------------------------------

#[ignore]
#[tokio::test]
async fn e2e_view_tree() -> Result<()> {
    let (s3, bucket, aws_client) = make_s3_client()?;
    ensure_bucket(&aws_client, bucket.as_str()).await?;

    let (db, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-tree-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-tree-host".to_string();

    setup_scan_fixture(&s3, &bucket, &db, &prefix, &host_id).await?;

    let view = LocalView::new(db);
    let tree = view.build_tree(&host_id, "").await?;

    // Verify tree structure: root should have children
    assert!(!tree.children.is_empty(), "tree should have children");

    // Find the prefix directory in the tree
    let prefix_dir = tree.children.iter().find(|n| n.name == prefix);
    assert!(
        prefix_dir.is_some(),
        "tree should contain prefix directory: {}",
        prefix
    );

    // Prefix dir should have photos and docs
    if let Some(dir) = prefix_dir {
        assert!(
            dir.children.iter().any(|n| n.name == "photos"),
            "should have photos dir"
        );
        assert!(
            dir.children.iter().any(|n| n.name == "docs"),
            "should have docs dir"
        );
    }

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_view_ls() -> Result<()> {
    let (s3, bucket, aws_client) = make_s3_client()?;
    ensure_bucket(&aws_client, bucket.as_str()).await?;

    let (db, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-ls-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-ls-host".to_string();

    setup_scan_fixture(&s3, &bucket, &db, &prefix, &host_id).await?;

    let view = LocalView::new(db);

    // List files in the photos/2024 subdirectory
    let photos_2024_prefix = format!("{prefix}/photos/2024");
    let entries = view
        .list_directory(
            &host_id,
            &photos_2024_prefix,
            SortField::Name,
            SortOrder::Ascending,
        )
        .await?;
    assert_eq!(entries.len(), 2, "photos/2024 should have 2 entries");
    assert!(
        entries.iter().any(|e| e.name == "vacation.jpg"),
        "should contain vacation.jpg"
    );
    assert!(
        entries.iter().any(|e| e.name == "party.mp4"),
        "should contain party.mp4"
    );

    // List files in docs
    let docs_prefix = format!("{prefix}/docs");
    let docs_entries = view
        .list_directory(
            &host_id,
            &docs_prefix,
            SortField::Name,
            SortOrder::Ascending,
        )
        .await?;
    assert_eq!(docs_entries.len(), 1, "docs should have 1 entry");
    assert_eq!(docs_entries[0].name, "readme.txt");

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_view_stat() -> Result<()> {
    let (s3, bucket, aws_client) = make_s3_client()?;
    ensure_bucket(&aws_client, bucket.as_str()).await?;

    let (db, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-stat-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-stat-host".to_string();

    setup_scan_fixture(&s3, &bucket, &db, &prefix, &host_id).await?;

    let view = LocalView::new(db);
    let stats = view.get_stats(&host_id).await?;

    assert_eq!(stats.total_files, 4, "should have 4 files total");

    // Check that by_file_type contains jpeg entries
    let jpeg_count = stats.by_file_type.get("jpeg").copied().unwrap_or(0);
    assert_eq!(jpeg_count, 2, "should have 2 jpeg files");

    // Check categories
    assert!(
        stats.by_category.contains_key("image"),
        "should have image category"
    );

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Task 2: view search, duplicates, timeline
// ---------------------------------------------------------------------------

#[ignore]
#[tokio::test]
async fn e2e_view_search() -> Result<()> {
    let (s3, bucket, aws_client) = make_s3_client()?;
    ensure_bucket(&aws_client, bucket.as_str()).await?;

    let (db, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-search-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-search-host".to_string();

    setup_scan_fixture(&s3, &bucket, &db, &prefix, &host_id).await?;

    let view = LocalView::new(db);

    // Search by name — should find vacation.jpg
    let result = view.search_by_name(&host_id, "vacation").await?;
    assert_eq!(
        result.total_count, 1,
        "should find 1 file matching 'vacation'"
    );
    assert!(
        result.files[0].key.as_str().contains("vacation.jpg"),
        "should match vacation.jpg"
    );

    // Search by name — should NOT find non-existent file
    let no_match = view.search_by_name(&host_id, "nonexistent").await?;
    assert_eq!(
        no_match.total_count, 0,
        "should find no files matching 'nonexistent'"
    );

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_view_duplicates() -> Result<()> {
    let (s3, bucket, aws_client) = make_s3_client()?;
    ensure_bucket(&aws_client, bucket.as_str()).await?;

    let (db, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-dup-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-dup-host".to_string();

    // The two JPEG files in setup_scan_fixture have identical content,
    // so they should be detected as duplicates
    setup_scan_fixture(&s3, &bucket, &db, &prefix, &host_id).await?;

    let view = LocalView::new(db);
    let groups = view.find_duplicates(&host_id).await?;

    // The two JPEG files have identical content, so should be detected as duplicates
    let has_duplicates = groups.iter().any(|g| g.files.len() >= 2);
    assert!(
        has_duplicates,
        "should detect at least one duplicate group with >=2 files"
    );

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_view_timeline() -> Result<()> {
    let (s3, bucket, aws_client) = make_s3_client()?;
    ensure_bucket(&aws_client, bucket.as_str()).await?;

    let (db, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-time-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-time-host".to_string();

    setup_scan_fixture(&s3, &bucket, &db, &prefix, &host_id).await?;

    let view = LocalView::new(db);
    let timeline = view.get_timeline(&host_id).await?;

    // Timeline should have at least one entry (the scan date)
    assert!(
        !timeline.is_empty(),
        "timeline should have at least one entry"
    );

    // Each entry should have a date and count
    for entry in &timeline {
        assert!(
            !entry.date.is_empty(),
            "each timeline entry should have a date"
        );
        assert!(
            entry.count > 0,
            "each timeline entry should have a positive count"
        );
    }

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Task 3: db push/pull, db lock/unlock
// ---------------------------------------------------------------------------

#[ignore]
#[tokio::test]
async fn e2e_db_push_pull() -> Result<()> {
    let (s3, bucket, aws_client) = make_s3_client()?;
    ensure_bucket(&aws_client, bucket.as_str()).await?;

    let (db, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-dbpush-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-dbpush-host".to_string();

    setup_scan_fixture(&s3, &bucket, &db, &prefix, &host_id).await?;

    // Push DB to remote (simulate db push)
    // Force a WAL checkpoint so the main DB file contains all committed data.
    sqlx::query("PRAGMA wal_checkpoint(FULL)")
        .execute(db.get_sqlite_connection_pool())
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to checkpoint: {e}")))?;
    let db_path = _dir.path().join("e2e-cli-test.db");
    let db_data = std::fs::read(&db_path).map_err(S3GalleryError::IoError)?;

    // Upload to a unique remote path to avoid conflicts
    let remote_db_key = ObjectKey::new(format!("{prefix}/e2e-cli-test.db"))?;
    s3.put_object(&bucket, &remote_db_key, &db_data).await?;

    // Pull DB from remote (simulate db pull)
    let pulled_data = s3.get_object(&bucket, &remote_db_key).await?;
    assert_eq!(
        pulled_data.len(),
        db_data.len(),
        "pulled DB should match local DB size"
    );

    // Verify the pulled data is a valid SQLite DB by reading from it
    let pulled_dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let pulled_db_path = pulled_dir.path().join("pulled.db");
    std::fs::write(&pulled_db_path, &pulled_data).map_err(S3GalleryError::IoError)?;
    // The pulled DB already has the schema and data from the original DB,
    // so we just open it directly without running migrations again.
    let pulled_db = create_pool(&pulled_db_path).await?;

    // Verify both DBs have the same file count
    let files =
        s3_gallery_core::db::models::FileEntry::count(db.get_sqlite_connection_pool(), &host_id)
            .await?;
    let pulled_files = s3_gallery_core::db::models::FileEntry::count(
        pulled_db.get_sqlite_connection_pool(),
        &host_id,
    )
    .await?;
    assert_eq!(pulled_files, files, "pulled DB should have same file count");

    // Cleanup
    s3.delete_object(&bucket, &remote_db_key).await?;
    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_db_lock() -> Result<()> {
    let (s3, bucket, aws_client) = make_s3_client()?;
    ensure_bucket(&aws_client, bucket.as_str()).await?;

    let prefix = format!("e2e-cli-lock-{}", uuid::Uuid::new_v4());
    let lock_key = ObjectKey::new(format!("{prefix}/s3-gallery.lock"))?;

    // Check lock is free initially
    let locked = s3_gallery_core::s3::lock::check_lock(s3.as_ref(), &bucket, &lock_key).await?;
    assert!(!locked, "lock should be free initially");

    // Acquire lock
    let guard = s3_gallery_core::s3::lock::acquire_lock(
        s3.clone(),
        bucket.clone(),
        lock_key.clone(),
        "e2e-test-client".to_string(),
    )
    .await?;
    assert!(
        s3_gallery_core::s3::lock::check_lock(s3.as_ref(), &bucket, &lock_key).await?,
        "lock should be held after acquisition"
    );

    // Release lock
    guard.release().await?;
    assert!(
        !s3_gallery_core::s3::lock::check_lock(s3.as_ref(), &bucket, &lock_key).await?,
        "lock should be free after release"
    );

    // Cleanup
    let _ = s3.delete_object(&bucket, &lock_key).await;
    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}
