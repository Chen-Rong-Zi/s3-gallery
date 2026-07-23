use crate::error::{Result, S3GalleryError};
use crate::s3::client::S3Client;
use crate::types::{BucketName, ObjectKey};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Lock lease data stored in OSS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockLease {
    pub client_id: String,
    pub acquired_at: String,
    pub expires_at: String,
}

impl LockLease {
    /// Create a new lease with 2-hour expiry
    pub fn new(client_id: String) -> Self {
        let now = Utc::now();
        let expires = now + Duration::hours(2);
        Self {
            client_id,
            acquired_at: now.to_rfc3339(),
            expires_at: expires.to_rfc3339(),
        }
    }

    /// Check if the lease has expired
    pub fn is_expired(&self) -> bool {
        match DateTime::parse_from_rfc3339(&self.expires_at) {
            Ok(dt) => dt < Utc::now(),
            Err(_) => true, // treat unparseable dates as expired
        }
    }
}

/// Lock guard — must be explicitly released.
/// #[must_use] ensures the compiler warns if it's dropped without calling release().
#[must_use = "LockGuard must be explicitly released via .release()"]
pub struct LockGuard {
    s3: Arc<dyn S3Client>,
    bucket: BucketName,
    lock_key: ObjectKey,
    client_id: String,
}

impl LockGuard {
    /// Create a new LockGuard (internal — use acquire_lock instead)
    pub(super) fn new(
        s3: Arc<dyn S3Client>,
        bucket: BucketName,
        lock_key: ObjectKey,
        client_id: String,
    ) -> Self {
        Self {
            s3,
            bucket,
            lock_key,
            client_id,
        }
    }

    /// Release the lock and consume the guard.
    /// After this call, the guard can no longer be used (ownership consumed).
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::S3Error` if the S3 delete operation fails.
    pub async fn release(self) -> Result<()> {
        tracing::debug!(
            target: "s3_gallery::lock",
            lock_key = %self.lock_key,
            client_id = %self.client_id,
            "Lock released"
        );
        self.s3.delete_object(&self.bucket, &self.lock_key).await?;
        Ok(())
    }

    /// Renew the lock (extend expiry by 2 hours from now).
    /// Requires &mut self to prevent concurrent renewals.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::Internal` if the lease fails to serialize.
    /// Returns `S3GalleryError::S3Error` if the S3 put operation fails.
    pub async fn renew(&mut self) -> Result<()> {
        let lease = LockLease::new(self.client_id.clone());
        let body = serde_json::to_vec(&lease)
            .map_err(|e| S3GalleryError::Internal(format!("Failed to serialize lease: {}", e)))?;
        self.s3
            .put_object(&self.bucket, &self.lock_key, &body)
            .await?;
        tracing::debug!(
            target: "s3_gallery::lock",
            lock_key = %self.lock_key,
            client_id = %self.client_id,
            "Lock renewed"
        );
        Ok(())
    }
}

/// Acquire a lock atomically.
/// Returns Ok(LockGuard) on success.
/// Returns Err(LockContention) if the lock is held by another client.
///
/// # Errors
///
/// Returns `S3GalleryError::Internal` if the lease fails to serialize.
/// Returns `S3GalleryError::S3Error` if the S3 put operation fails.
/// Returns `S3GalleryError::LockContention` if the lock is already held by another client.
pub async fn acquire_lock(
    s3: Arc<dyn S3Client>,
    bucket: BucketName,
    lock_key: ObjectKey,
    client_id: String,
) -> Result<LockGuard> {
    let lease = LockLease::new(client_id.clone());
    let body = serde_json::to_vec(&lease)
        .map_err(|e| S3GalleryError::Internal(format!("Failed to serialize lease: {}", e)))?;

    let acquired = s3
        .put_object_if_none_match(&bucket, &lock_key, &body)
        .await?;

    if acquired {
        tracing::info!(
            target: "s3_gallery::lock",
            lock_key = %lock_key,
            client_id = %client_id,
            "Lock acquired"
        );
        Ok(LockGuard::new(s3, bucket, lock_key, client_id))
    } else {
        tracing::warn!(
            target: "s3_gallery::lock",
            lock_key = %lock_key,
            client_id = %client_id,
            "Lock contention — held by another client"
        );
        Err(S3GalleryError::LockContention(
            "Lock is held by another client".to_string(),
        ))
    }
}

/// Check if a lock exists (without acquiring it)
///
/// # Errors
///
/// Returns `S3GalleryError::S3Error` if the S3 operation fails.
pub async fn check_lock(
    s3: &dyn S3Client,
    bucket: &BucketName,
    lock_key: &ObjectKey,
) -> Result<bool> {
    s3.object_exists(bucket, lock_key).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s3::mock::MockS3Client;
    use crate::types::BucketName;
    use crate::types::ObjectKey;
    use std::sync::Arc;

    fn test_key(name: &str) -> ObjectKey {
        ObjectKey::new(name)
            .ok()
            .unwrap_or_else(|| ObjectKey::new("test-key").ok().unwrap_or_else(|| loop {}))
    }

    fn test_bucket(name: &str) -> BucketName {
        BucketName::new(name).ok().unwrap_or_else(|| {
            BucketName::new("test-bucket")
                .ok()
                .unwrap_or_else(|| loop {})
        })
    }

    #[tokio::test]
    async fn test_acquire_and_release_lock() -> Result<()> {
        let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let bucket = test_bucket("test-bucket");
        let lock_key = test_key("s3-gallery.lock");
        let client_id = "test-client-1".to_string();

        let guard = acquire_lock(s3.clone(), bucket.clone(), lock_key.clone(), client_id).await?;
        assert!(check_lock(s3.as_ref(), &bucket, &lock_key).await?);
        guard.release().await?;
        assert!(!check_lock(s3.as_ref(), &bucket, &lock_key).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_lock_contention() -> Result<()> {
        let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let bucket = test_bucket("test-bucket");
        let lock_key = test_key("s3-gallery.lock");

        let _guard = acquire_lock(
            s3.clone(),
            bucket.clone(),
            lock_key.clone(),
            "client-1".to_string(),
        )
        .await?;

        let result = acquire_lock(
            s3.clone(),
            bucket.clone(),
            lock_key.clone(),
            "client-2".to_string(),
        )
        .await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_renew_lock() -> Result<()> {
        let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let bucket = test_bucket("test-bucket");
        let lock_key = test_key("s3-gallery.lock");

        let mut guard = acquire_lock(
            s3.clone(),
            bucket.clone(),
            lock_key.clone(),
            "client-1".to_string(),
        )
        .await?;
        guard.renew().await?;
        guard.release().await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_lock_lease_expiry() {
        let lease = LockLease::new("test-client".to_string());
        assert!(!lease.is_expired());

        // Create an expired lease
        let mut expired_lease = LockLease::new("test-client".to_string());
        expired_lease.expires_at = "2020-01-01T00:00:00Z".to_string();
        assert!(expired_lease.is_expired());

        // Test invalid date
        let mut bad_lease = LockLease::new("test-client".to_string());
        bad_lease.expires_at = "not-a-valid-date".to_string();
        assert!(bad_lease.is_expired());
    }

    #[tokio::test]
    async fn test_acquire_lock_after_release() -> Result<()> {
        let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let bucket = test_bucket("test-bucket");
        let lock_key = test_key("s3-gallery.lock");

        // First acquire and release
        let guard = acquire_lock(
            s3.clone(),
            bucket.clone(),
            lock_key.clone(),
            "client-1".to_string(),
        )
        .await?;
        guard.release().await?;

        // Acquire again should succeed
        let guard2 = acquire_lock(
            s3.clone(),
            bucket.clone(),
            lock_key.clone(),
            "client-2".to_string(),
        )
        .await?;
        guard2.release().await?;
        Ok(())
    }
}
