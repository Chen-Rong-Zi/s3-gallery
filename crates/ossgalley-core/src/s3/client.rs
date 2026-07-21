use async_trait::async_trait;

use crate::error::Result;
use crate::types::{BucketName, Etag, FileSize, ObjectKey};

/// Summary of an S3 object returned by list operations.
#[derive(Debug, Clone)]
pub struct ObjectSummary {
    pub key: ObjectKey,
    pub etag: Etag,
    pub size: FileSize,
    pub last_modified: String,
}

/// Metadata for a single object.
#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    pub etag: Etag,
    pub size: FileSize,
    pub content_type: Option<String>,
    pub last_modified: String,
}

/// S3Client trait -- abstract over real S3 and mock.
///
/// All methods are async and fallible.  Implementations must be `Send + Sync +
/// 'static` so they can be used with `dyn S3Client` in shared state.
#[async_trait]
pub trait S3Client: Send + Sync + 'static {
    /// List objects with a given prefix.  Returns all matching objects.
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::S3Error` if the S3 operation fails.
    async fn list_objects(
        &self,
        bucket: &BucketName,
        prefix: &ObjectKey,
    ) -> Result<Vec<ObjectSummary>>;

    /// Get metadata for a single object (HEAD).
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::ObjectNotFound` if the key does not exist.
    async fn head_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<ObjectMetadata>;

    /// Get full object content (GET).
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::ObjectNotFound` if the key does not exist.
    async fn get_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<Vec<u8>>;

    /// Get a byte range of an object (GET with Range header).
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::ObjectNotFound` if the key does not exist, or
    /// `OssgalleyError::S3Error` if the range is invalid.
    async fn get_object_range(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        start: u64,
        end: u64,
    ) -> Result<Vec<u8>>;

    /// Put an object (PUT).
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::S3Error` if the S3 operation fails.
    async fn put_object(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<()>;

    /// Put an object only if it doesn't exist (PUT with If-None-Match: *).
    ///
    /// Returns `true` if the object was created, `false` if it already existed.
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::S3Error` if the S3 operation fails.
    async fn put_object_if_none_match(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<bool>;

    /// Delete an object (DELETE).
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::S3Error` if the S3 operation fails.
    async fn delete_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<()>;

    /// Check if an object exists (HEAD with 404 handling).
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::S3Error` if the S3 operation fails.
    async fn object_exists(&self, bucket: &BucketName, key: &ObjectKey) -> Result<bool>;
}