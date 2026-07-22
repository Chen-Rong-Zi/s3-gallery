# CLI E2E Test Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add 8 e2e tests for CLI commands (scan, view, db) against a real MinIO instance.

**Architecture:** Each test uploads test files to a real MinIO → runs scan via `run_scan()` → runs view/db operations via library API → asserts results → cleans up. All tests use `#[ignore]` and run on demand.

**Tech Stack:** Rust, tokio, sqlx, aws-sdk-s3, uuid, tempfile

---

### Task 1: Create test file with helpers and view tree/ls/stat tests

**Files:**
- Create: `tests/e2e_cli_test.rs`

- [ ] **Step 1: Write the test file with shared helpers**

Create `tests/e2e_cli_test.rs` with the following helpers and three tests:

```rust
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

use s3_gallery_core::db::models::FileEntry;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::scan::scanner::{run_scan, ScanConfig};
use s3_gallery_core::types::{BucketName, ObjectKey, SortField, SortOrder};
use s3_gallery_core::view::LocalView;
use sqlx::SqlitePool;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

async fn setup_e2e_db() -> Result<(SqlitePool, TempDir)> {
    let dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let db_path = dir.path().join("e2e-cli-test.db");
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;
    Ok((pool, dir))
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
            Err(S3GalleryError::S3Error(format!("Failed to create bucket: {msg}")))
        }
    }
}

fn make_s3_client() -> Result<(Arc<dyn S3Client>, BucketName)> {
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
    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    Ok((s3, bucket))
}

/// Upload test files to a unique prefix, scan them, and return the pool + LocalView + prefix for cleanup.
async fn setup_scan_fixture(
    s3: &Arc<dyn S3Client>,
    bucket: &BucketName,
    pool: &SqlitePool,
    prefix: &str,
    host_id: &str,
) -> Result<()> {
    let test_files = vec![
        (format!("{prefix}/photos/2024/vacation.jpg"), vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09]),
        (format!("{prefix}/photos/2024/party.mp4"), vec![0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x6D, 0x70, 0x34, 0x32, 0x00, 0x00, 0x00, 0x00, 0x6D, 0x70, 0x34, 0x32]),
        (format!("{prefix}/photos/2023/old-photo.jpg"), vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09]),
        (format!("{prefix}/docs/readme.txt"), b"Hello, this is a readme file.".to_vec()),
    ];

    for (key, data) in &test_files {
        let object_key = ObjectKey::new(key)?;
        s3.put_object(bucket, &object_key, data).await?;
    }

    let scan_config = ScanConfig {
        host_id: host_id.to_string(),
        s3: s3.clone(),
        db: pool.clone(),
        bucket: bucket.clone(),
        prefix: ObjectKey::new(prefix)?,
        concurrency: 4,
        extract_metadata: false,
        generate_thumbnails: false,
        client_id: "e2e-cli-test".to_string(),
    };

    let result = run_scan(scan_config).await?;
    assert_eq!(result.total_files, 4, "should find all 4 test files");
    assert_eq!(result.new_files, 4);

    Ok(())
}

async fn cleanup_prefix(s3: &Arc<dyn S3Client>, bucket: &BucketName, prefix: &str) -> Result<()> {
    let prefix_key = ObjectKey::new(prefix)?;
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
    let (s3, bucket) = make_s3_client()?;
    ensure_bucket(&s3_aws_client(&s3), bucket.as_str()).await?;

    let (pool, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-tree-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-tree-host".to_string();

    setup_scan_fixture(&s3, &bucket, &pool, &prefix, &host_id).await?;

    let view = LocalView::new(pool.clone());
    let tree = view.build_tree(&host_id, "").await?;

    // Verify tree structure: root should have one child (the prefix directory)
    assert!(!tree.children.is_empty(), "tree should have children");

    // Find the prefix directory in the tree
    let prefix_dir = tree.children.iter().find(|n| n.name == prefix);
    assert!(prefix_dir.is_some(), "tree should contain prefix directory");

    // Prefix dir should have photos and docs
    if let Some(dir) = prefix_dir {
        assert!(dir.children.iter().any(|n| n.name == "photos"), "should have photos dir");
        assert!(dir.children.iter().any(|n| n.name == "docs"), "should have docs dir");
    }

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

/// Helper to get the underlying AWS SDK client from the S3Client trait object.
fn s3_aws_client(s3: &Arc<dyn S3Client>) -> &aws_sdk_s3::Client {
    // Safety: We know the concrete type is RealS3Client
    let real = s3.as_any().downcast_ref::<RealS3Client>().expect("expected RealS3Client");
    &real.client
}

#[ignore]
#[tokio::test]
async fn e2e_view_ls() -> Result<()> {
    let (s3, bucket) = make_s3_client()?;
    ensure_bucket(&s3_aws_client(&s3), bucket.as_str()).await?;

    let (pool, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-ls-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-ls-host".to_string();

    setup_scan_fixture(&s3, &bucket, &pool, &prefix, &host_id).await?;

    let view = LocalView::new(pool.clone());

    // List files in the photos/2024 subdirectory
    let photos_2024_prefix = format!("{prefix}/photos/2024");
    let entries = view.list_directory(&host_id, &photos_2024_prefix, SortField::Name, SortOrder::Ascending).await?;
    assert_eq!(entries.len(), 2, "photos/2024 should have 2 entries");
    assert!(entries.iter().any(|e| e.name == "vacation.jpg"), "should contain vacation.jpg");
    assert!(entries.iter().any(|e| e.name == "party.mp4"), "should contain party.mp4");

    // List files in docs
    let docs_prefix = format!("{prefix}/docs");
    let docs_entries = view.list_directory(&host_id, &docs_prefix, SortField::Name, SortOrder::Ascending).await?;
    assert_eq!(docs_entries.len(), 1, "docs should have 1 entry");
    assert_eq!(docs_entries[0].name, "readme.txt");

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_view_stat() -> Result<()> {
    let (s3, bucket) = make_s3_client()?;
    ensure_bucket(&s3_aws_client(&s3), bucket.as_str()).await?;

    let (pool, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-stat-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-stat-host".to_string();

    setup_scan_fixture(&s3, &bucket, &pool, &prefix, &host_id).await?;

    let view = LocalView::new(pool.clone());
    let stats = view.get_stats(&host_id).await?;

    assert_eq!(stats.total_files, 4, "should have 4 files total");
    // 2 jpg + 1 mp4 + 1 txt = 4 files
    // The file types are classified by extension via the classifier

    // Check that by_file_type contains jpeg entries
    let jpeg_count = stats.by_file_type.get("jpeg").copied().unwrap_or(0);
    assert_eq!(jpeg_count, 2, "should have 2 jpeg files");

    // Check categories
    assert!(stats.by_category.contains_key("image"), "should have image category");

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}
```

- [ ] **Step 2: Build and verify**

Run: `cargo test --test e2e_cli_test -- --ignored 2>&1 | grep -E '^test result:|FAILED'`
Expected: 3 tests should pass

- [ ] **Step 3: Commit**

```bash
git add tests/e2e_cli_test.rs
git commit -m "feat: add e2e tests for view tree, ls, and stat"
```

---

### Task 2: Add search, duplicates, and timeline e2e tests

**Files:**
- Modify: `tests/e2e_cli_test.rs`

- [ ] **Step 1: Add three new tests to the existing file**

Append the following tests after the existing ones in `tests/e2e_cli_test.rs`:

```rust
// ---------------------------------------------------------------------------
// Task 2: view search, duplicates, timeline
// ---------------------------------------------------------------------------

#[ignore]
#[tokio::test]
async fn e2e_view_search() -> Result<()> {
    let (s3, bucket) = make_s3_client()?;
    ensure_bucket(&s3_aws_client(&s3), bucket.as_str()).await?;

    let (pool, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-search-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-search-host".to_string();

    setup_scan_fixture(&s3, &bucket, &pool, &prefix, &host_id).await?;

    let view = LocalView::new(pool.clone());

    // Search by name — should find vacation.jpg
    let result = view.search_by_name(&host_id, "vacation").await?;
    assert_eq!(result.total_count, 1, "should find 1 file matching 'vacation'");
    assert!(result.files[0].key.contains("vacation.jpg"), "should match vacation.jpg");

    // Search by name — should NOT find non-existent file
    let no_match = view.search_by_name(&host_id, "nonexistent").await?;
    assert_eq!(no_match.total_count, 0, "should find no files matching 'nonexistent'");

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_view_duplicates() -> Result<()> {
    let (s3, bucket) = make_s3_client()?;
    ensure_bucket(&s3_aws_client(&s3), bucket.as_str()).await?;

    let (pool, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-dup-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-dup-host".to_string();

    // Upload files with same content (vacation.jpg and old-photo.jpg have same bytes)
    // But the setup_scan_fixture already has identical content for these two files
    setup_scan_fixture(&s3, &bucket, &pool, &prefix, &host_id).await?;

    let view = LocalView::new(pool.clone());
    let groups = view.find_duplicates(&host_id).await?;

    // The two JPEG files have identical content (setup_scan_fixture uses same bytes)
    // So they should be detected as duplicates
    let has_duplicates = groups.iter().any(|g| g.files.len() >= 2);
    assert!(has_duplicates, "should detect at least one duplicate group");

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_view_timeline() -> Result<()> {
    let (s3, bucket) = make_s3_client()?;
    ensure_bucket(&s3_aws_client(&s3), bucket.as_str()).await?;

    let (pool, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-time-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-time-host".to_string();

    setup_scan_fixture(&s3, &bucket, &pool, &prefix, &host_id).await?;

    let view = LocalView::new(pool.clone());
    let timeline = view.get_timeline(&host_id).await?;

    // Timeline should have at least one entry (the scan date)
    assert!(!timeline.is_empty(), "timeline should have at least one entry");

    // Each entry should have a date and count
    for entry in &timeline {
        assert!(!entry.date.is_empty(), "each timeline entry should have a date");
        assert!(entry.count > 0, "each timeline entry should have a positive count");
    }

    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}
```

- [ ] **Step 2: Build and verify**

Run: `cargo test --test e2e_cli_test -- --ignored 2>&1 | grep -E '^test result:|FAILED'`
Expected: 6 tests should pass

- [ ] **Step 3: Commit**

```bash
git add tests/e2e_cli_test.rs
git commit -m "feat: add e2e tests for view search, duplicates, and timeline"
```

---

### Task 3: Add db push/pull and lock e2e tests

**Files:**
- Modify: `tests/e2e_cli_test.rs`

- [ ] **Step 1: Add database push/pull test**

```rust
// ---------------------------------------------------------------------------
// Task 3: db push/pull, db lock/unlock
// ---------------------------------------------------------------------------

#[ignore]
#[tokio::test]
async fn e2e_db_push_pull() -> Result<()> {
    let (s3, bucket) = make_s3_client()?;
    ensure_bucket(&s3_aws_client(&s3), bucket.as_str()).await?;

    let (pool, _dir) = setup_e2e_db().await?;
    let prefix = format!("e2e-cli-dbpush-{}", uuid::Uuid::new_v4());
    let host_id = "e2e-dbpush-host".to_string();

    setup_scan_fixture(&s3, &bucket, &pool, &prefix, &host_id).await?;

    // Push DB to remote (simulate db push)
    // Read the local DB file and upload to S3
    let db_path = _dir.path().join("e2e-cli-test.db");
    let db_data = std::fs::read(&db_path).map_err(S3GalleryError::IoError)?;
    let db_key = ObjectKey::new("e2e-cli-test.db")?;

    // Upload to a unique remote path to avoid conflicts
    let remote_db_key = ObjectKey::new(format!("{prefix}/e2e-cli-test.db"))?;
    s3.put_object(&bucket, &remote_db_key, &db_data).await?;

    // Pull DB from remote (simulate db pull)
    let pulled_data = s3.get_object(&bucket, &remote_db_key).await?;
    assert_eq!(pulled_data.len(), db_data.len(), "pulled DB should match local DB size");

    // Verify the pulled data is a valid SQLite DB by creating a pool from it
    let pulled_dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let pulled_db_path = pulled_dir.path().join("pulled.db");
    std::fs::write(&pulled_db_path, &pulled_data).map_err(S3GalleryError::IoError)?;
    let pulled_pool = create_pool(&pulled_db_path).await?;
    run_migrations(&pulled_pool).await?;

    // Verify both DBs have the same file count
    let files = s3_gallery_core::db::models::FileEntry::count(&pool, &host_id).await?;
    let pulled_files = s3_gallery_core::db::models::FileEntry::count(&pulled_pool, &host_id).await?;
    assert_eq!(pulled_files, files, "pulled DB should have same file count");

    // Cleanup
    s3.delete_object(&bucket, &remote_db_key).await?;
    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_db_lock() -> Result<()> {
    let (s3, bucket) = make_s3_client()?;
    ensure_bucket(&s3_aws_client(&s3), bucket.as_str()).await?;

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
    ).await?;
    assert!(s3_gallery_core::s3::lock::check_lock(s3.as_ref(), &bucket, &lock_key).await?,
        "lock should be held after acquisition");

    // Release lock
    guard.release().await?;
    assert!(!s3_gallery_core::s3::lock::check_lock(s3.as_ref(), &bucket, &lock_key).await?,
        "lock should be free after release");

    // Cleanup
    let _ = s3.delete_object(&bucket, &lock_key).await;
    cleanup_prefix(&s3, &bucket, &prefix).await?;
    Ok(())
}
```

- [ ] **Step 2: Build and verify**

Run: `cargo test --test e2e_cli_test -- --ignored 2>&1 | grep -E '^test result:|FAILED'`
Expected: 8 tests should pass

- [ ] **Step 3: Commit**

```bash
git add tests/e2e_cli_test.rs
git commit -m "feat: add e2e tests for db push/pull and lock/unlock"
```

---

### Task 4: Final build and verify

**Files:**
- Build: entire project

- [ ] **Step 1: Full build**

Run: `cargo build 2>&1 | grep -E '^error'`
Expected: no errors

- [ ] **Step 2: Run all non-e2e tests**

Run: `cargo test 2>&1 | grep -E '^test result:'`
Expected: all pass

- [ ] **Step 3: Run all e2e tests against real MinIO**

Run: `S3_ENDPOINT=https://localhost:9000 AWS_ACCESS_KEY_ID=s3oss AWS_SECRET_ACCESS_KEY=s3oss1234 S3_BUCKET=ossgalley-e2e-test cargo test --test e2e_cli_test -- --ignored 2>&1`
Expected: 8 tests pass

- [ ] **Step 4: Commit spec and plan**

```bash
git add docs/superpowers/specs/2026-07-22-cli-e2e-test-design.md docs/superpowers/plans/2026-07-22-cli-e2e-test-plan.md
git commit -m "docs: add CLI e2e test spec and implementation plan"
```