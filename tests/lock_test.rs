//! Integration tests for the S3-based distributed lock.
//!
//! Tests lock acquisition, renewal, release, contention detection, and
//! the must-use / consumption semantics of LockGuard.

use std::sync::Arc;

use ossgalley_core::error::Result;
use ossgalley_core::s3::client::S3Client;
use ossgalley_core::s3::lock::{acquire_lock, check_lock};
use ossgalley_core::s3::mock::MockS3Client;
use ossgalley_core::types::{BucketName, ObjectKey};

mod common;

/// Helper to create a test lock key.
fn lock_key() -> ObjectKey {
    ObjectKey::new("test-host/.ossgallery/db.lock").expect("valid lock key")
}

#[tokio::test]
async fn test_lock_acquire_and_release() -> Result<()> {
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket = common::test_bucket()?;
    let key = lock_key();

    // Acquire
    let guard = acquire_lock(s3.clone(), bucket.clone(), key.clone(), "client-1".to_string()).await?;

    // Lock should exist
    assert!(check_lock(s3.as_ref(), &bucket, &key).await?);

    // Release
    guard.release().await?;

    // Lock should be gone
    assert!(!check_lock(s3.as_ref(), &bucket, &key).await?);
    Ok(())
}

#[tokio::test]
async fn test_lock_contention_is_prevented() -> Result<()> {
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket = common::test_bucket()?;
    let key = lock_key();

    // First client acquires the lock.
    let _guard1 = acquire_lock(s3.clone(), bucket.clone(), key.clone(), "client-1".to_string()).await?;

    // Second client should fail with LockContention.
    let result = acquire_lock(s3.clone(), bucket.clone(), key.clone(), "client-2".to_string()).await;
    assert!(result.is_err(), "second acquisition should fail");

    match result {
        Err(err) => {
            let err_str = err.to_string();
            assert!(
                err_str.contains("Lock contention") || err_str.contains("contention"),
                "error should mention contention: {err_str}"
            );
        }
        Ok(_) => panic!("expected Err, got Ok"),
    }
    Ok(())
}

#[tokio::test]
async fn test_lock_renewal() -> Result<()> {
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket = common::test_bucket()?;
    let key = lock_key();

    let mut guard = acquire_lock(s3.clone(), bucket.clone(), key.clone(), "client-1".to_string()).await?;
    guard.renew().await?;
    guard.release().await?;
    Ok(())
}

#[tokio::test]
async fn test_lock_acquire_after_release() -> Result<()> {
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket = common::test_bucket()?;
    let key = lock_key();

    // Acquire and release by client 1.
    let guard = acquire_lock(s3.clone(), bucket.clone(), key.clone(), "client-1".to_string()).await?;
    guard.release().await?;

    // Acquire by client 2 should succeed.
    let guard2 = acquire_lock(s3.clone(), bucket.clone(), key.clone(), "client-2".to_string()).await?;
    guard2.release().await?;
    Ok(())
}

#[tokio::test]
async fn test_lock_guard_must_be_consumed() -> Result<()> {
    // The #[must_use] attribute on LockGuard ensures the compiler warns
    // if the guard is dropped without calling .release().  This test verifies
    // that the guard type is correctly consumed by release().
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket = common::test_bucket()?;
    let key = lock_key();

    let guard = acquire_lock(s3.clone(), bucket.clone(), key.clone(), "client-1".to_string()).await?;
    guard.release().await?;

    // After release, the lock should be gone.
    assert!(!check_lock(s3.as_ref(), &bucket, &key).await?);
    Ok(())
}

#[tokio::test]
async fn test_lock_release_consumes_guard() -> Result<()> {
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket = common::test_bucket()?;
    let key = lock_key();

    let guard = acquire_lock(s3.clone(), bucket.clone(), key.clone(), "client-1".to_string()).await?;

    // guard.release() consumes self, so we can't use guard after this line.
    guard.release().await?;

    // Verify we can acquire again (lock was released).
    let guard2 = acquire_lock(s3.clone(), bucket.clone(), key.clone(), "client-2".to_string()).await?;
    guard2.release().await?;
    Ok(())
}

#[tokio::test]
async fn test_lock_with_different_keys_are_independent() -> Result<()> {
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket = common::test_bucket()?;
    let key1 = ObjectKey::new("host-a/.ossgallery/db.lock")?;
    let key2 = ObjectKey::new("host-b/.ossgallery/db.lock")?;

    // Acquire locks on different keys concurrently.
    let _guard_a = acquire_lock(s3.clone(), bucket.clone(), key1.clone(), "client-a".to_string()).await?;
    let _guard_b = acquire_lock(s3.clone(), bucket.clone(), key2.clone(), "client-b".to_string()).await?;

    // Both locks should exist.
    assert!(check_lock(s3.as_ref(), &bucket, &key1).await?);
    assert!(check_lock(s3.as_ref(), &bucket, &key2).await?);

    _guard_a.release().await?;
    _guard_b.release().await?;
    Ok(())
}

#[tokio::test]
async fn test_lock_multiple_renewals() -> Result<()> {
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket = common::test_bucket()?;
    let key = lock_key();

    let mut guard = acquire_lock(s3.clone(), bucket.clone(), key.clone(), "client-1".to_string()).await?;

    // Renew multiple times.
    guard.renew().await?;
    guard.renew().await?;
    guard.renew().await?;

    guard.release().await?;

    // Verify lock is released.
    assert!(!check_lock(s3.as_ref(), &bucket, &key).await?);
    Ok(())
}

#[tokio::test]
async fn test_check_lock_on_empty_bucket() -> Result<()> {
    let s3 = MockS3Client::new();
    let bucket = common::test_bucket()?;
    let key = lock_key();

    // No lock acquired, should not exist.
    let exists = check_lock(&s3, &bucket, &key).await?;
    assert!(!exists);
    Ok(())
}

#[tokio::test]
async fn test_acquire_lock_with_different_bucket() -> Result<()> {
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket_a = BucketName::new("bucket-a")?;
    let bucket_b = BucketName::new("bucket-b")?;
    let key_a = ObjectKey::new("host-a/.ossgallery/db.lock")?;
    let key_b = ObjectKey::new("host-b/.ossgallery/db.lock")?;

    // Locks on different buckets are independent in production.
    // The MockS3Client ignores the bucket parameter, so we use different keys
    // to simulate the same behavior.
    let _guard_a = acquire_lock(s3.clone(), bucket_a.clone(), key_a.clone(), "client-a".to_string()).await?;
    let _guard_b = acquire_lock(s3.clone(), bucket_b.clone(), key_b.clone(), "client-b".to_string()).await?;

    assert!(check_lock(s3.as_ref(), &bucket_a, &key_a).await?);
    assert!(check_lock(s3.as_ref(), &bucket_b, &key_b).await?);

    _guard_a.release().await?;
    _guard_b.release().await?;
    Ok(())
}