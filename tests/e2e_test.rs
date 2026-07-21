//! End-to-end tests against a real MinIO instance.
//!
//! These tests are marked with `#[ignore]` so they only run on demand
//! (`cargo test -- --ignored`).  They connect to a real MinIO server
//! configured via environment variables:
//!
//! - `S3_ENDPOINT` (default: `http://localhost:9000`)
//! - `AWS_ACCESS_KEY_ID` (default: `minioadmin`)
//! - `AWS_SECRET_ACCESS_KEY` (default: `minioadmin`)
//! - `S3_BUCKET` (default: `ossgalley-e2e-test`)
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

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::primitives::ByteStream;
use sqlx::SqlitePool;
use tempfile::TempDir;

use ossgalley_core::db::models::FileEntry;
use ossgalley_core::db::pool::create_pool;
use ossgalley_core::db::schema::run_migrations;
use ossgalley_core::error::{OssgalleyError, Result};
use ossgalley_core::s3::client::S3Client;
use ossgalley_core::s3::lock::{acquire_lock, check_lock};
use ossgalley_core::scan::scanner::{run_scan, ScanConfig};
use ossgalley_core::types::{BucketName, ObjectKey};

// ---------------------------------------------------------------------------
// E2E S3 client wrapper
// ---------------------------------------------------------------------------

/// A real S3 client that wraps `aws_sdk_s3::Client` and implements the
/// `S3Client` trait.
struct RealS3Client {
    client: aws_sdk_s3::Client,
    bucket: BucketName,
}

impl RealS3Client {
    /// Build a new client from environment variables.
    #[allow(deprecated)]
    async fn from_env() -> Result<Self> {
        let endpoint = env_or("S3_ENDPOINT", "http://localhost:9000");
        let access_key = env_or("AWS_ACCESS_KEY_ID", "minioadmin");
        let secret_key = env_or("AWS_SECRET_ACCESS_KEY", "minioadmin");
        let region_str = env_or("S3_REGION", "us-east-1");
        let bucket = BucketName::new(env_or("S3_BUCKET", "ossgalley-e2e-test"))?;

        let creds = Credentials::new(access_key, secret_key, None, None, "e2e-test");

        let config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::v2025_01_17())
            .region(Region::new(region_str))
            .endpoint_url(&endpoint)
            .credentials_provider(creds)
            .force_path_style(true)
            .build();

        let client = aws_sdk_s3::Client::from_conf(config);

        Ok(Self { client, bucket })
    }
}

#[async_trait::async_trait]
impl S3Client for RealS3Client {
    async fn list_objects(
        &self,
        _bucket: &BucketName,
        prefix: &ObjectKey,
    ) -> Result<Vec<ossgalley_core::s3::client::ObjectSummary>> {
        let resp = self
            .client
            .list_objects_v2()
            .bucket(self.bucket.as_str())
            .prefix(prefix.as_str())
            .send()
            .await
            .map_err(|e| OssgalleyError::S3Error(e.to_string()))?;

        let mut results = Vec::new();
        if let Some(contents) = resp.contents {
            for obj in contents {
                let key = obj.key().unwrap_or_default();
                if key.is_empty() {
                    continue;
                }
                let etag = obj.e_tag().unwrap_or("unknown").trim_matches('"');
                results.push(ossgalley_core::s3::client::ObjectSummary {
                    key: ObjectKey::new(key)
                        .map_err(|e| OssgalleyError::S3Error(e.to_string()))?,
                    etag: ossgalley_core::types::Etag::new(etag)
                        .map_err(|e| OssgalleyError::S3Error(e.to_string()))?,
                    size: ossgalley_core::types::FileSize::new(obj.size().unwrap_or(0) as u64),
                    last_modified: obj
                        .last_modified()
                        .map(|d| d.to_string())
                        .unwrap_or_default(),
                });
            }
        }
        Ok(results)
    }

    async fn head_object(
        &self,
        _bucket: &BucketName,
        key: &ObjectKey,
    ) -> Result<ossgalley_core::s3::client::ObjectMetadata> {
        let resp = self
            .client
            .head_object()
            .bucket(self.bucket.as_str())
            .key(key.as_str())
            .send()
            .await
            .map_err(|e| OssgalleyError::S3Error(e.to_string()))?;

        let etag = resp.e_tag().unwrap_or("unknown").trim_matches('"');
        Ok(ossgalley_core::s3::client::ObjectMetadata {
            etag: ossgalley_core::types::Etag::new(etag)
                .map_err(|e| OssgalleyError::S3Error(e.to_string()))?,
            size: ossgalley_core::types::FileSize::new(
    resp.content_length().unwrap_or(0).max(0) as u64,
),
            content_type: resp.content_type().map(|s| s.to_string()),
            last_modified: resp
                .last_modified()
                .map(|d| d.to_string())
                .unwrap_or_default(),
        })
    }

    async fn get_object(
        &self,
        _bucket: &BucketName,
        key: &ObjectKey,
    ) -> Result<Vec<u8>> {
        let resp = self
            .client
            .get_object()
            .bucket(self.bucket.as_str())
            .key(key.as_str())
            .send()
            .await
            .map_err(|e| OssgalleyError::S3Error(e.to_string()))?;

        let data = resp
            .body
            .collect()
            .await
            .map_err(|e| OssgalleyError::S3Error(e.to_string()))?;
        Ok(data.to_vec())
    }

    async fn get_object_range(
        &self,
        _bucket: &BucketName,
        key: &ObjectKey,
        start: u64,
        end: u64,
    ) -> Result<Vec<u8>> {
        let range = format!("bytes={}-{}", start, end - 1);
        let resp = self
            .client
            .get_object()
            .bucket(self.bucket.as_str())
            .key(key.as_str())
            .range(range)
            .send()
            .await
            .map_err(|e| OssgalleyError::S3Error(e.to_string()))?;

        let data = resp
            .body
            .collect()
            .await
            .map_err(|e| OssgalleyError::S3Error(e.to_string()))?;
        Ok(data.to_vec())
    }

    async fn put_object(
        &self,
        _bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<()> {
        self.client
            .put_object()
            .bucket(self.bucket.as_str())
            .key(key.as_str())
            .body(ByteStream::from(body.to_vec()))
            .send()
            .await
            .map_err(|e| OssgalleyError::S3Error(e.to_string()))?;
        Ok(())
    }

    async fn put_object_if_none_match(
        &self,
        _bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<bool> {
        let result = self
            .client
            .put_object()
            .bucket(self.bucket.as_str())
            .key(key.as_str())
            .body(ByteStream::from(body.to_vec()))
            .if_none_match("*")
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(err) => {
                // PreconditionFailed (412) means the object already exists.
                if let Some(service_err) = err.as_service_error() {
                    if service_err.code() == Some("PreconditionFailed") {
                        return Ok(false);
                    }
                }
                Err(OssgalleyError::S3Error(err.to_string()))
            }
        }
    }

    async fn delete_object(
        &self,
        _bucket: &BucketName,
        key: &ObjectKey,
    ) -> Result<()> {
        self.client
            .delete_object()
            .bucket(self.bucket.as_str())
            .key(key.as_str())
            .send()
            .await
            .map_err(|e| OssgalleyError::S3Error(e.to_string()))?;
        Ok(())
    }

    async fn object_exists(
        &self,
        _bucket: &BucketName,
        key: &ObjectKey,
    ) -> Result<bool> {
        let result = self
            .client
            .head_object()
            .bucket(self.bucket.as_str())
            .key(key.as_str())
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(err) => {
                // Check if the error is "Not Found" (404) via the service error code.
                if let Some(service_err) = err.as_service_error() {
                    if service_err.code() == Some("NotFound") {
                        return Ok(false);
                    }
                }
                Err(OssgalleyError::S3Error(err.to_string()))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read an environment variable with a default value.
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Set up a temporary database with migrations.
async fn setup_e2e_db() -> Result<(SqlitePool, TempDir)> {
    let dir = tempfile::tempdir()
        .map_err(|e| OssgalleyError::DbError(e.to_string()))?;
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
    let create_result = client
        .create_bucket()
        .bucket(bucket_name)
        .send()
        .await;
    match create_result {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            // If the bucket already exists, that's fine.
            if msg.contains("BucketAlreadyExists") || msg.contains("BucketAlreadyOwnedByYou") {
                return Ok(());
            }
            Err(OssgalleyError::S3Error(format!("Failed to create bucket: {msg}")))
        }
    }
}

// ---------------------------------------------------------------------------
// E2E Tests (all #[ignore])
// ---------------------------------------------------------------------------

#[ignore]
#[tokio::test]
async fn e2e_s3_put_and_get_object() -> Result<()> {
    let real = RealS3Client::from_env().await?;
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "ossgalley-e2e-test"))?;
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
    let real = RealS3Client::from_env().await?;
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "ossgalley-e2e-test"))?;

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
    let real = RealS3Client::from_env().await?;
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "ossgalley-e2e-test"))?;

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
    let real = RealS3Client::from_env().await?;
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "ossgalley-e2e-test"))?;

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
    let real = RealS3Client::from_env().await?;
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "ossgalley-e2e-test"))?;

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
    let real = RealS3Client::from_env().await?;
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "ossgalley-e2e-test"))?;

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
    let real = RealS3Client::from_env().await?;
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "ossgalley-e2e-test"))?;
    let (pool, _dir) = setup_e2e_db().await?;

    // Use prefix WITHOUT trailing slash to avoid double-slash in lock key
    let prefix = format!("e2e-scan-{}", uuid::Uuid::new_v4());
    let key = ObjectKey::new(format!("{prefix}/test.jpg"))?;
    s3.put_object(&bucket, &key, b"fake image data").await?;

    let scan_config = ScanConfig {
        s3: s3.clone(),
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

    let entry = FileEntry::get_by_key(&pool, key.as_str()).await?;
    assert_eq!(entry.file_type, "jpeg");
    assert!(!entry.is_deleted);

    s3.delete_object(&bucket, &key).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn e2e_lock_and_release() -> Result<()> {
    let real = RealS3Client::from_env().await?;
    ensure_bucket(&real.client, real.bucket.as_str()).await?;

    let s3 = Arc::new(real) as Arc<dyn S3Client>;
    let bucket = BucketName::new(env_or("S3_BUCKET", "ossgalley-e2e-test"))?;

    let lock_key = ObjectKey::new(format!(
        "e2e-lock-{}/.ossgallery/db.lock",
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