//! End-to-end tests against a real MinIO instance.
//!
//! These tests are marked with `#[ignore]` so they only run on demand
//! (`cargo test -- --ignored`).  They connect to a real MinIO server
//! configured via environment variables:
//!
//! - `S3_ENDPOINT` (default: `http://localhost:9000`)
//! - `AWS_ACCESS_KEY_ID` (default: `minioadmin`)
//! - `AWS_SECRET_ACCESS_KEY` (default: `minioadmin`)
//! - `S3_BUCKET` (default: `s3-gallery-e2e-test`)
//! - `S3_REGION` (default: `us-east-1`)
//!
//! # Prerequisites
//!
//! A MinIO instance must be running at the configured endpoint.  The quickest
//! way is:
//!
//! ```bash
//! docker run -p 9000:9000 -p 9001:9001 \
//!   -e MINIO_ROOT_USER=minioadmin \
//!   -e MINIO_ROOT_PASSWORD=minioadmin \
//!   quay.io/minio/minio server /data --console-address ":9001"
//! ```
//!
//! Then create the bucket and run:
//! ```bash
//! cargo test --test e2e_test -- --ignored
//! ```

use std::sync::Arc;

use sqlx::SqlitePool;
use tempfile::TempDir;

use s3_gallery_core::db::models::FileEntry;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::lock::{acquire_lock, check_lock};
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::scan::scanner::{run_scan, ScanConfig};
use s3_gallery_core::types::{BucketName, ObjectKey};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read an environment variable with a default value.
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Set up a temporary database with migrations.
async fn setup_e2e_db() -> Result<(SqlitePool, TempDir)> {
    let dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let db_path = dir.path().join("e2e-test.db");
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;
    Ok((pool, dir))
}

/// Ensure the test bucket exists.  If it doesn't, try to create it.
async fn ensure_bucket(client: &aws_sdk_s3::Client, bucket_name: &str) -> Result<()> {
    let result = client.head_bucket().bucket(bucket_name).send().await;

    if result.is_ok() {
        return Ok(());
    }

    // If head_bucket fails, try to create it.  BucketAlreadyExists / BucketAlreadyOwnedByYou
    // are treated as success (race with another test run).
    let create_result = client.create_bucket().bucket(bucket_name).send().await;
    match create_result {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            // If the bucket already exists, that's fine.
            if msg.contains("BucketAlreadyExists") || msg.contains("BucketAlreadyOwnedByYou") {
                return Ok(());
            }
            Err(S3GalleryError::S3Error(format!(
                "Failed to create bucket: {msg}"
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// E2E Tests (all #[ignore])
// ---------------------------------------------------------------------------

#[ignore]
#[tokio::test]
async fn e2e_s3_put_and_get_object() -> Result<()> {
    let config = OssConfig::validate(
        BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?,
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_REGION", "us-east-1"),
        &env_or("AWS_ACCESS_KEY_ID", "minioadmin"),
        &env_or("AWS_SECRET_ACCESS_KEY", "minioadmin"),
        4,
    )?;
    let real = RealS3Client::from_config(&config);
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?;
    let key = ObjectKey::new(format!("e2e-put-get-{}.txt", uuid::Uuid::new_v4()))?;
    let body = b"Hello from E2E test!";

    s3.put_object(&bucket, &key, body).await?;
    let result = s3.get_object(&bucket, &key).await?;
    assert_eq!(result, body);

    s3.delete_object(&bucket, &key).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_s3_list_objects() -> Result<()> {
    let config = OssConfig::validate(
        BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?,
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_REGION", "us-east-1"),
        &env_or("AWS_ACCESS_KEY_ID", "minioadmin"),
        &env_or("AWS_SECRET_ACCESS_KEY", "minioadmin"),
        4,
    )?;
    let real = RealS3Client::from_config(&config);
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?;

    let prefix = format!("e2e-list-{}/", uuid::Uuid::new_v4());
    let key1 = ObjectKey::new(format!("{prefix}file-a.txt"))?;
    let key2 = ObjectKey::new(format!("{prefix}file-b.txt"))?;

    s3.put_object(&bucket, &key1, b"content a").await?;
    s3.put_object(&bucket, &key2, b"content b").await?;

    let prefix_key = ObjectKey::new(&prefix)?;
    let results = s3.list_objects(&bucket, &prefix_key).await?;
    assert_eq!(results.len(), 2);

    s3.delete_object(&bucket, &key1).await?;
    s3.delete_object(&bucket, &key2).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_s3_head_object() -> Result<()> {
    let config = OssConfig::validate(
        BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?,
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_REGION", "us-east-1"),
        &env_or("AWS_ACCESS_KEY_ID", "minioadmin"),
        &env_or("AWS_SECRET_ACCESS_KEY", "minioadmin"),
        4,
    )?;
    let real = RealS3Client::from_config(&config);
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?;

    let key = ObjectKey::new(format!("e2e-head-{}.txt", uuid::Uuid::new_v4()))?;
    s3.put_object(&bucket, &key, b"test data").await?;

    let meta = s3.head_object(&bucket, &key).await?;
    assert_eq!(meta.size.as_u64(), 9);
    assert!(!meta.last_modified.is_empty());

    s3.delete_object(&bucket, &key).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_s3_object_exists() -> Result<()> {
    let config = OssConfig::validate(
        BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?,
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_REGION", "us-east-1"),
        &env_or("AWS_ACCESS_KEY_ID", "minioadmin"),
        &env_or("AWS_SECRET_ACCESS_KEY", "minioadmin"),
        4,
    )?;
    let real = RealS3Client::from_config(&config);
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?;

    let key = ObjectKey::new(format!("e2e-exists-{}.txt", uuid::Uuid::new_v4()))?;

    assert!(!s3.object_exists(&bucket, &key).await?);

    s3.put_object(&bucket, &key, b"data").await?;
    assert!(s3.object_exists(&bucket, &key).await?);

    s3.delete_object(&bucket, &key).await?;
    assert!(!s3.object_exists(&bucket, &key).await?);
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_s3_get_object_range() -> Result<()> {
    let config = OssConfig::validate(
        BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?,
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_REGION", "us-east-1"),
        &env_or("AWS_ACCESS_KEY_ID", "minioadmin"),
        &env_or("AWS_SECRET_ACCESS_KEY", "minioadmin"),
        4,
    )?;
    let real = RealS3Client::from_config(&config);
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?;

    let key = ObjectKey::new(format!("e2e-range-{}.txt", uuid::Uuid::new_v4()))?;
    let body = b"Hello, World!";
    s3.put_object(&bucket, &key, body).await?;

    let range = s3.get_object_range(&bucket, &key, 0, 5).await?;
    assert_eq!(range, b"Hello");

    s3.delete_object(&bucket, &key).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_s3_put_if_none_match() -> Result<()> {
    let config = OssConfig::validate(
        BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?,
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_REGION", "us-east-1"),
        &env_or("AWS_ACCESS_KEY_ID", "minioadmin"),
        &env_or("AWS_SECRET_ACCESS_KEY", "minioadmin"),
        4,
    )?;
    let real = RealS3Client::from_config(&config);
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?;

    let key = ObjectKey::new(format!("e2e-if-none-match-{}.txt", uuid::Uuid::new_v4()))?;

    let created = s3
        .put_object_if_none_match(&bucket, &key, b"original")
        .await?;
    assert!(created);

    let created = s3
        .put_object_if_none_match(&bucket, &key, b"updated")
        .await?;
    assert!(!created);

    s3.delete_object(&bucket, &key).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_scan_real_bucket() -> Result<()> {
    let config = OssConfig::validate(
        BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?,
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_REGION", "us-east-1"),
        &env_or("AWS_ACCESS_KEY_ID", "minioadmin"),
        &env_or("AWS_SECRET_ACCESS_KEY", "minioadmin"),
        4,
    )?;
    let real = RealS3Client::from_config(&config);
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?;
    let (pool, _dir) = setup_e2e_db().await?;

    // Use prefix WITHOUT trailing slash to avoid double-slash in lock key
    let prefix = format!("e2e-scan-{}", uuid::Uuid::new_v4());
    let key = ObjectKey::new(format!("{prefix}/test.jpg"))?;
    s3.put_object(&bucket, &key, b"fake image data").await?;

    let scan_config = ScanConfig {
        host_id: "e2e-test-host".to_string(),
        s3: S3Service::new(s3.clone()),
        db: pool.clone(),
        bucket: bucket.clone(),
        prefix: ObjectKey::new(&prefix)?,
        concurrency: 4,
        extract_metadata: false,
        generate_thumbnails: false,
        client_id: "e2e-test-client".to_string(),
    };

    let result = run_scan(scan_config).await?;
    assert_eq!(result.total_files, 1, "should find the test object");
    assert_eq!(result.new_files, 1);

    let entry = FileEntry::get_by_key(&pool, "e2e-test-host", key.as_str()).await?;
    assert_eq!(entry.file_type, "jpeg");
    assert!(!entry.is_deleted);

    s3.delete_object(&bucket, &key).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_lock_and_release() -> Result<()> {
    let config = OssConfig::validate(
        BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?,
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_REGION", "us-east-1"),
        &env_or("AWS_ACCESS_KEY_ID", "minioadmin"),
        &env_or("AWS_SECRET_ACCESS_KEY", "minioadmin"),
        4,
    )?;
    let real = RealS3Client::from_config(&config);
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "s3-gallery-e2e-test"))?;

    let lock_key = ObjectKey::new(format!(
        "e2e-lock-{}/.s3-gallery/db.lock",
        uuid::Uuid::new_v4()
    ))?;

    let guard = acquire_lock(
        s3.clone(),
        bucket.clone(),
        lock_key.clone(),
        "e2e-client".to_string(),
    )
    .await?;

    assert!(check_lock(s3.as_ref(), &bucket, &lock_key).await?);

    guard.release().await?;

    assert!(!check_lock(s3.as_ref(), &bucket, &lock_key).await?);

    let _ = s3.delete_object(&bucket, &lock_key).await;
    Ok(())
}
