//! Logging wrapper around an S3Client.
//!
//! `LoggedS3Client` delegates every method to an inner `Arc<dyn S3Client>` and
//! logs each call with timing, bucket, key, and result.  Each method uses its
//! own `tracing` target (e.g. `s3_gallery::s3::get_object`) so log output can
//! be controlled per-method via `RUST_LOG`.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tracing::{debug, info, warn};

use crate::error::Result;
use crate::types::{BucketName, ObjectKey};

use super::client::{ObjectMetadata, ObjectSummary, S3Client};

/// Logging wrapper around an S3Client.
///
/// Wraps any `Arc<dyn S3Client>` and logs every S3 operation with:
/// - target (per-method, e.g. `s3_gallery::s3::get_object`)
/// - bucket, key
/// - elapsed time in milliseconds
/// - success (debug level) or failure (warn level)
/// - method-specific fields (size, count, etc.)
#[derive(Clone)]
pub struct LoggedS3Client {
    inner: Arc<dyn S3Client>,
}

impl std::fmt::Debug for LoggedS3Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoggedS3Client")
            .field("inner", &"<dyn S3Client>")
            .finish()
    }
}

impl LoggedS3Client {
    /// Wrap an S3Client with logging.
    #[must_use]
    pub fn new(inner: Arc<dyn S3Client>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl S3Client for LoggedS3Client {
    async fn list_objects(
        &self,
        bucket: &BucketName,
        prefix: &ObjectKey,
    ) -> Result<Vec<ObjectSummary>> {
        let start = Instant::now();
        let result = self.inner.list_objects(bucket, prefix).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match &result {
            Ok(objects) => {
                debug!(
                    target: "s3_gallery::s3::list_objects",
                    bucket = %bucket,
                    prefix = %prefix,
                    elapsed_ms,
                    count = objects.len(),
                    "list_objects succeeded",
                );
            }
            Err(err) => {
                warn!(
                    target: "s3_gallery::s3::list_objects",
                    bucket = %bucket,
                    prefix = %prefix,
                    elapsed_ms,
                    error = %err,
                    "list_objects failed",
                );
            }
        }

        result
    }

    async fn head_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<ObjectMetadata> {
        let start = Instant::now();
        let result = self.inner.head_object(bucket, key).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match &result {
            Ok(meta) => {
                debug!(
                    target: "s3_gallery::s3::head_object",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    size = meta.size.as_u64(),
                    "head_object succeeded",
                );
            }
            Err(err) => {
                warn!(
                    target: "s3_gallery::s3::head_object",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    error = %err,
                    "head_object failed",
                );
            }
        }

        result
    }

    async fn get_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<Vec<u8>> {
        let start = Instant::now();
        let result = self.inner.get_object(bucket, key).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match &result {
            Ok(body) => {
                debug!(
                    target: "s3_gallery::s3::get_object",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    size = body.len(),
                    "get_object succeeded",
                );
            }
            Err(err) => {
                warn!(
                    target: "s3_gallery::s3::get_object",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    error = %err,
                    "get_object failed",
                );
            }
        }

        result
    }

    async fn get_object_range(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        start: u64,
        end: u64,
    ) -> Result<Vec<u8>> {
        let timer = Instant::now();
        let result = self.inner.get_object_range(bucket, key, start, end).await;
        let elapsed_ms = timer.elapsed().as_millis() as u64;

        match &result {
            Ok(body) => {
                debug!(
                    target: "s3_gallery::s3::get_object_range",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    range_start = start,
                    range_end = end,
                    size = body.len(),
                    "get_object_range succeeded",
                );
            }
            Err(err) => {
                warn!(
                    target: "s3_gallery::s3::get_object_range",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    range_start = start,
                    range_end = end,
                    error = %err,
                    "get_object_range failed",
                );
            }
        }

        result
    }

    async fn put_object(&self, bucket: &BucketName, key: &ObjectKey, body: &[u8]) -> Result<()> {
        let start = Instant::now();
        let result = self.inner.put_object(bucket, key, body).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match &result {
            Ok(()) => {
                debug!(
                    target: "s3_gallery::s3::put_object",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    size = body.len(),
                    "put_object succeeded",
                );
            }
            Err(err) => {
                warn!(
                    target: "s3_gallery::s3::put_object",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    size = body.len(),
                    error = %err,
                    "put_object failed",
                );
            }
        }

        result
    }

    async fn put_object_if_none_match(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<bool> {
        let start = Instant::now();
        let result = self.inner.put_object_if_none_match(bucket, key, body).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match &result {
            Ok(created) => {
                if *created {
                    debug!(
                        target: "s3_gallery::s3::put_object_if_none_match",
                        bucket = %bucket,
                        key = %key,
                        elapsed_ms,
                        size = body.len(),
                        created = true,
                        "put_object_if_none_match succeeded — created",
                    );
                } else {
                    info!(
                        target: "s3_gallery::s3::put_object_if_none_match",
                        bucket = %bucket,
                        key = %key,
                        elapsed_ms,
                        size = body.len(),
                        created = false,
                        "put_object_if_none_match — object already exists",
                    );
                }
            }
            Err(err) => {
                warn!(
                    target: "s3_gallery::s3::put_object_if_none_match",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    size = body.len(),
                    error = %err,
                    "put_object_if_none_match failed",
                );
            }
        }

        result
    }

    async fn delete_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<()> {
        let start = Instant::now();
        let result = self.inner.delete_object(bucket, key).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match &result {
            Ok(()) => {
                debug!(
                    target: "s3_gallery::s3::delete_object",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    "delete_object succeeded",
                );
            }
            Err(err) => {
                warn!(
                    target: "s3_gallery::s3::delete_object",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    error = %err,
                    "delete_object failed",
                );
            }
        }

        result
    }

    async fn object_exists(&self, bucket: &BucketName, key: &ObjectKey) -> Result<bool> {
        let start = Instant::now();
        let result = self.inner.object_exists(bucket, key).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match &result {
            Ok(exists) => {
                debug!(
                    target: "s3_gallery::s3::object_exists",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    exists = exists,
                    "object_exists succeeded",
                );
            }
            Err(err) => {
                warn!(
                    target: "s3_gallery::s3::object_exists",
                    bucket = %bucket,
                    key = %key,
                    elapsed_ms,
                    error = %err,
                    "object_exists failed",
                );
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::error::S3GalleryError;
    use crate::types::{BucketName, ObjectKey};

    use super::super::mock::MockS3Client;
    use super::*;

    #[tokio::test]
    async fn test_logged_client_delegates_get_object() -> Result<()> {
        let mock = MockS3Client::with_fixtures(vec![("test.txt", b"hello world")])?;
        let logged = LoggedS3Client::new(Arc::new(mock));
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        let result = logged.get_object(&bucket, &key).await?;
        assert_eq!(result, b"hello world");
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_list_objects() -> Result<()> {
        let mock = MockS3Client::with_fixtures(vec![
            ("photos/a.jpg", b"img1"),
            ("photos/b.jpg", b"img2"),
            ("docs/readme.txt", b"text"),
        ])?;
        let logged = LoggedS3Client::new(Arc::new(mock));
        let bucket = BucketName::new("test-bucket")?;
        let prefix = ObjectKey::new("photos/")?;

        let results = logged.list_objects(&bucket, &prefix).await?;
        assert_eq!(results.len(), 2);
        for summary in &results {
            assert!(summary.key.as_str().starts_with("photos/"));
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_head_object() -> Result<()> {
        let mock = MockS3Client::with_fixtures(vec![("test.txt", b"hello world")])?;
        let logged = LoggedS3Client::new(Arc::new(mock));
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        let meta = logged.head_object(&bucket, &key).await?;
        assert_eq!(meta.size.as_u64(), 11);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_get_object_range() -> Result<()> {
        let mock = MockS3Client::with_fixtures(vec![("test.txt", b"hello world")])?;
        let logged = LoggedS3Client::new(Arc::new(mock));
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        let range = logged.get_object_range(&bucket, &key, 0, 5).await?;
        assert_eq!(range, b"hello");
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_put_object() -> Result<()> {
        let mock = MockS3Client::new();
        let logged = LoggedS3Client::new(Arc::new(mock));
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;
        let body = b"hello world";

        logged.put_object(&bucket, &key, body).await?;

        // Verify by reading it back
        let result = logged.get_object(&bucket, &key).await?;
        assert_eq!(result, body);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_put_if_none_match() -> Result<()> {
        let mock = MockS3Client::new();
        let logged = LoggedS3Client::new(Arc::new(mock));
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;
        let body = b"original";

        // First put should create
        let created = logged.put_object_if_none_match(&bucket, &key, body).await?;
        assert!(created);

        // Second put should not create
        let created = logged
            .put_object_if_none_match(&bucket, &key, b"updated")
            .await?;
        assert!(!created);

        // Verify original content is preserved
        let result = logged.get_object(&bucket, &key).await?;
        assert_eq!(result, body);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_delete_object() -> Result<()> {
        let mock = MockS3Client::with_fixtures(vec![("test.txt", b"hello")])?;
        let logged = LoggedS3Client::new(Arc::new(mock));
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test.txt")?;

        assert!(logged.object_exists(&bucket, &key).await?);
        logged.delete_object(&bucket, &key).await?;
        assert!(!logged.object_exists(&bucket, &key).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_delegates_object_exists() -> Result<()> {
        let mock = MockS3Client::with_fixtures(vec![("exists.txt", b"hello")])?;
        let logged = LoggedS3Client::new(Arc::new(mock));
        let bucket = BucketName::new("test-bucket")?;

        let exists_key = ObjectKey::new("exists.txt")?;
        let missing_key = ObjectKey::new("missing.txt")?;

        assert!(logged.object_exists(&bucket, &exists_key).await?);
        assert!(!logged.object_exists(&bucket, &missing_key).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_logged_client_propagates_errors() -> Result<()> {
        let mock = MockS3Client::new();
        let logged = LoggedS3Client::new(Arc::new(mock));
        let bucket = BucketName::new("test-bucket")?;
        let missing_key = ObjectKey::new("nonexistent.txt")?;

        let err = logged.get_object(&bucket, &missing_key).await.unwrap_err();
        assert!(matches!(err, S3GalleryError::ObjectNotFound(_)));

        let err = logged.head_object(&bucket, &missing_key).await.unwrap_err();
        assert!(matches!(err, S3GalleryError::ObjectNotFound(_)));

        Ok(())
    }
}
