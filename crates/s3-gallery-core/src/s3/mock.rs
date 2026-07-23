use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::{Result, S3GalleryError};
use crate::types::{BucketName, Etag, FileSize, ObjectKey};

use super::client::{ObjectMetadata, ObjectSummary, S3Client};

// ---------------------------------------------------------------------------
// MockObject -- internal storage entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct MockObject {
    body: Vec<u8>,
    etag: Etag,
    content_type: Option<String>,
    last_modified: String,
}

// ---------------------------------------------------------------------------
// MockS3Client
// ---------------------------------------------------------------------------

/// In-memory mock S3 client for testing.
///
/// Stores objects in a `HashMap<String, MockObject>` behind `Arc<Mutex<...>>`
/// so the client is cheap to clone and can be shared across tasks.
#[derive(Debug, Clone)]
pub struct MockS3Client {
    storage: Arc<Mutex<HashMap<String, MockObject>>>,
}

impl MockS3Client {
    /// Create a new empty mock client.
    #[must_use]
    pub fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a mock client pre-populated with the given fixtures.
    ///
    /// Each entry is a `(key, body)` pair.  A mock ETag is generated
    /// automatically for each object.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::ValidationError` if any key is invalid.
    pub fn with_fixtures(objects: Vec<(&str, &[u8])>) -> Result<Self> {
        let mut storage = HashMap::new();
        for (key, body) in objects {
            let object_key = ObjectKey::new(key)?;
            let etag = generate_mock_etag()?;
            storage.insert(
                object_key.as_str().to_string(),
                MockObject {
                    body: body.to_vec(),
                    etag,
                    content_type: None,
                    last_modified: chrono::Utc::now().to_rfc3339(),
                },
            );
        }
        Ok(Self {
            storage: Arc::new(Mutex::new(storage)),
        })
    }

    /// Add an object to the mock store.
    ///
    /// A mock ETag is generated automatically.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::ValidationError` if the key cannot be
    /// converted to an `ObjectKey`.
    pub async fn add_object(&self, key: &str, body: Vec<u8>) -> Result<()> {
        let object_key = ObjectKey::new(key)?;
        let etag = generate_mock_etag()?;
        let mut storage = self.storage.lock().await;
        storage.insert(
            object_key.as_str().to_string(),
            MockObject {
                body,
                etag,
                content_type: None,
                last_modified: chrono::Utc::now().to_rfc3339(),
            },
        );
        Ok(())
    }
}

impl Default for MockS3Client {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// S3Client trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl S3Client for MockS3Client {
    async fn list_objects(
        &self,
        _bucket: &BucketName,
        prefix: &ObjectKey,
    ) -> Result<Vec<ObjectSummary>> {
        let storage = self.storage.lock().await;
        let prefix_str = prefix.as_str();
        let mut results = Vec::new();

        for (key_str, obj) in storage.iter() {
            if key_str.starts_with(prefix_str) {
                results.push(ObjectSummary {
                    key: ObjectKey::new(key_str.clone())?,
                    etag: obj.etag.clone(),
                    size: FileSize::new(obj.body.len() as u64),
                    last_modified: obj.last_modified.clone(),
                });
            }
        }

        Ok(results)
    }

    async fn head_object(&self, _bucket: &BucketName, key: &ObjectKey) -> Result<ObjectMetadata> {
        let storage = self.storage.lock().await;
        let obj = storage
            .get(key.as_str())
            .ok_or_else(|| S3GalleryError::ObjectNotFound(key.to_string()))?;

        Ok(ObjectMetadata {
            etag: obj.etag.clone(),
            size: FileSize::new(obj.body.len() as u64),
            content_type: obj.content_type.clone(),
            last_modified: obj.last_modified.clone(),
        })
    }

    async fn get_object(&self, _bucket: &BucketName, key: &ObjectKey) -> Result<Vec<u8>> {
        let storage = self.storage.lock().await;
        let obj = storage
            .get(key.as_str())
            .ok_or_else(|| S3GalleryError::ObjectNotFound(key.to_string()))?;

        Ok(obj.body.clone())
    }

    async fn get_object_range(
        &self,
        _bucket: &BucketName,
        key: &ObjectKey,
        start: u64,
        end: u64,
    ) -> Result<Vec<u8>> {
        let storage = self.storage.lock().await;
        let obj = storage
            .get(key.as_str())
            .ok_or_else(|| S3GalleryError::ObjectNotFound(key.to_string()))?;

        let start_us = usize::try_from(start)
            .map_err(|e| S3GalleryError::S3Error(format!("invalid start offset: {e}")))?;
        let end_us = usize::try_from(end)
            .map_err(|e| S3GalleryError::S3Error(format!("invalid end offset: {e}")))?;

        let slice = obj
            .body
            .get(start_us..end_us)
            .ok_or_else(|| S3GalleryError::S3Error("invalid byte range".to_string()))?;

        Ok(slice.to_vec())
    }

    async fn put_object(&self, _bucket: &BucketName, key: &ObjectKey, body: &[u8]) -> Result<()> {
        let etag = generate_mock_etag()?;
        let mut storage = self.storage.lock().await;
        storage.insert(
            key.as_str().to_string(),
            MockObject {
                body: body.to_vec(),
                etag,
                content_type: None,
                last_modified: chrono::Utc::now().to_rfc3339(),
            },
        );
        Ok(())
    }

    async fn put_object_if_none_match(
        &self,
        _bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<bool> {
        let mut storage = self.storage.lock().await;
        if storage.contains_key(key.as_str()) {
            return Ok(false);
        }

        let etag = generate_mock_etag()?;
        storage.insert(
            key.as_str().to_string(),
            MockObject {
                body: body.to_vec(),
                etag,
                content_type: None,
                last_modified: chrono::Utc::now().to_rfc3339(),
            },
        );
        Ok(true)
    }

    async fn delete_object(&self, _bucket: &BucketName, key: &ObjectKey) -> Result<()> {
        let mut storage = self.storage.lock().await;
        storage.remove(key.as_str());
        Ok(())
    }

    async fn object_exists(&self, _bucket: &BucketName, key: &ObjectKey) -> Result<bool> {
        let storage = self.storage.lock().await;
        Ok(storage.contains_key(key.as_str()))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a mock ETag using a UUID v4.
///
/// UUID v4 strings are always non-empty and contain no surrounding quotes, so
/// `Etag::new` will never fail for this input.
fn generate_mock_etag() -> Result<Etag> {
    Etag::new(uuid::Uuid::new_v4().to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_put_and_get_object() -> Result<()> {
        let client = MockS3Client::new();
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test-file.txt")?;
        let body = b"Hello, World!";

        client.put_object(&bucket, &key, body).await?;
        let result = client.get_object(&bucket, &key).await?;

        assert_eq!(result, body);
        Ok(())
    }

    #[tokio::test]
    async fn test_list_objects_with_prefix() -> Result<()> {
        let client = MockS3Client::new();
        let bucket = BucketName::new("test-bucket")?;

        client
            .add_object("photos/2024/vacation.jpg", b"image data".to_vec())
            .await?;
        client
            .add_object("photos/2024/party.jpg", b"more data".to_vec())
            .await?;
        client
            .add_object("docs/readme.txt", b"text data".to_vec())
            .await?;

        let prefix = ObjectKey::new("photos/2024/")?;
        let results = client.list_objects(&bucket, &prefix).await?;

        assert_eq!(results.len(), 2);
        for summary in &results {
            let key_str = summary.key.as_str();
            assert!(
                key_str.starts_with("photos/2024/"),
                "unexpected key: {key_str}"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_head_object() -> Result<()> {
        let client = MockS3Client::new();
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test-file.txt")?;
        let body = b"Hello, World!";

        client.put_object(&bucket, &key, body).await?;
        let metadata = client.head_object(&bucket, &key).await?;

        assert_eq!(metadata.size.as_u64(), body.len() as u64);
        assert!(metadata.content_type.is_none());
        assert!(!metadata.last_modified.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_get_object_range() -> Result<()> {
        let client = MockS3Client::new();
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test-file.txt")?;
        let body = b"Hello, World!";

        client.put_object(&bucket, &key, body).await?;
        let range = client.get_object_range(&bucket, &key, 0, 5).await?;

        assert_eq!(range, b"Hello");
        Ok(())
    }

    #[tokio::test]
    async fn test_put_if_none_match() -> Result<()> {
        let client = MockS3Client::new();
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test-file.txt")?;
        let body = b"original content";

        // First put should succeed (object doesn't exist yet).
        let created = client.put_object_if_none_match(&bucket, &key, body).await?;
        assert!(created);

        // Second put should return false (object already exists).
        let created = client
            .put_object_if_none_match(&bucket, &key, b"updated content")
            .await?;
        assert!(!created);

        // Verify the original content is still there.
        let result = client.get_object(&bucket, &key).await?;
        assert_eq!(result, body);
        Ok(())
    }

    #[tokio::test]
    async fn test_delete_object() -> Result<()> {
        let client = MockS3Client::new();
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test-file.txt")?;
        let body = b"Hello, World!";

        client.put_object(&bucket, &key, body).await?;
        assert!(client.object_exists(&bucket, &key).await?);

        client.delete_object(&bucket, &key).await?;
        assert!(!client.object_exists(&bucket, &key).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_object_not_found() -> Result<()> {
        let client = MockS3Client::new();
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("nonexistent.txt")?;

        let err = client.get_object(&bucket, &key).await.unwrap_err();
        assert!(matches!(err, S3GalleryError::ObjectNotFound(_)));

        let err = client.head_object(&bucket, &key).await.unwrap_err();
        assert!(matches!(err, S3GalleryError::ObjectNotFound(_)));

        let exists = client.object_exists(&bucket, &key).await?;
        assert!(!exists);
        Ok(())
    }

    #[tokio::test]
    async fn test_with_fixtures() -> Result<()> {
        let client =
            MockS3Client::with_fixtures(vec![("a.txt", b"content a"), ("b.txt", b"content b")])?;
        let bucket = BucketName::new("test-bucket")?;

        let key_a = ObjectKey::new("a.txt")?;
        let key_b = ObjectKey::new("b.txt")?;

        assert!(client.object_exists(&bucket, &key_a).await?);
        assert!(client.object_exists(&bucket, &key_b).await?);

        let result = client.get_object(&bucket, &key_a).await?;
        assert_eq!(result, b"content a");
        Ok(())
    }
}
