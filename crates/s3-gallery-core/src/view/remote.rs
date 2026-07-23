use crate::db::models::*;
use crate::error::Result;
#[cfg(test)]
use crate::error::S3GalleryError;
use crate::s3::client::S3Client;
use crate::thumbnail::generator::generate_thumbnail;
use crate::types::*;
use chrono::Utc;
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing;

/// RemoteView — provides S3 I/O operations on top of LocalView.
/// The presence of Arc<dyn S3Client> signals that fetch_* methods
/// perform network requests.
pub struct RemoteView {
    db: SqlitePool,
    s3: Arc<dyn S3Client>,
    bucket: BucketName,
}

impl RemoteView {
    pub fn new(db: SqlitePool, s3: Arc<dyn S3Client>, bucket: BucketName) -> Self {
        Self { db, s3, bucket }
    }

    /// Fetch a file's content from S3.
    /// # Warning
    /// This method performs an S3 GET request — network I/O.
    /// # Errors
    /// Returns an error if the S3 request fails.
    pub async fn fetch_file_content(&self, key: &ObjectKey) -> Result<Vec<u8>> {
        let data = self.s3.get_object(&self.bucket, key).await?;
        tracing::debug!(
            target: "s3_gallery::remote_view",
            key = %key,
            size = data.len(),
            "Fetched file content from S3"
        );
        Ok(data)
    }

    /// Fetch a file thumbnail from S3, caching it locally.
    /// # Warning
    /// This method performs an S3 GET request if the thumbnail is not cached.
    /// # Errors
    /// Returns an error if the S3 request fails, thumbnail generation fails,
    /// or database operations fail.
    pub async fn fetch_thumbnail(&self, key: &ObjectKey) -> Result<Vec<u8>> {
        // Check local cache first
        if let Ok(entry) = ThumbnailEntry::get(&self.db, key.as_str()).await {
            tracing::debug!(
                target: "s3_gallery::remote_view",
                key = %key,
                "Thumbnail cache hit"
            );
            return Ok(entry.data);
        }

        // Fetch from S3 and generate thumbnail
        let data = self.s3.get_object(&self.bucket, key).await?;
        let thumbnail = generate_thumbnail(&data)?;

        // Cache locally
        ThumbnailEntry::insert(
            &self.db,
            &ThumbnailEntry {
                file_key: key.as_str().to_string(),
                data: thumbnail.clone(),
                format: "jpeg".to_string(),
                width: None,
                height: None,
                cached_at: Utc::now().to_rfc3339(),
            },
        )
        .await?;

        tracing::info!(
            target: "s3_gallery::remote_view",
            key = %key,
            "Thumbnail generated from S3"
        );

        Ok(thumbnail)
    }

    /// Fetch a byte range from S3 (for EXIF extraction).
    /// # Warning
    /// This method performs an S3 GET request with Range header.
    /// # Errors
    /// Returns an error if the S3 request fails.
    pub async fn fetch_byte_range(&self, key: &ObjectKey, start: u64, end: u64) -> Result<Vec<u8>> {
        let data = self
            .s3
            .get_object_range(&self.bucket, key, start, end)
            .await?;
        tracing::debug!(
            target: "s3_gallery::remote_view",
            key = %key,
            start = start,
            end = end,
            size = data.len(),
            "Fetched byte range from S3"
        );
        Ok(data)
    }

    /// Check if an object exists on S3.
    /// # Warning
    /// This method performs an S3 HEAD request.
    /// # Errors
    /// Returns an error if the S3 request fails.
    pub async fn fetch_object_exists(&self, key: &ObjectKey) -> Result<bool> {
        let exists = self.s3.object_exists(&self.bucket, key).await?;
        tracing::debug!(
            target: "s3_gallery::remote_view",
            key = %key,
            exists = exists,
            "Checked object existence on S3"
        );
        Ok(exists)
    }

    /// Get a reference to the underlying S3Client.
    pub fn s3(&self) -> &dyn S3Client {
        self.s3.as_ref()
    }

    /// Get a reference to the database pool.
    pub fn db(&self) -> &SqlitePool {
        &self.db
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::s3::mock::MockS3Client;
    use crate::types::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn setup_test_key(name: &str) -> ObjectKey {
        ObjectKey::new(name)
            .ok()
            .unwrap_or_else(|| ObjectKey::new("a").ok().unwrap_or_else(|| loop {}))
    }

    #[tokio::test]
    async fn test_fetch_file_content() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("test-file.txt")?;

        // Add test data to mock
        s3.put_object(&bucket, &key, b"hello world").await?;

        let view = RemoteView::new(pool, s3, bucket);
        let content = view.fetch_file_content(&key).await?;
        assert_eq!(content, b"hello world");
        Ok(())
    }

    #[tokio::test]
    async fn test_fetch_object_not_found() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
        let bucket = BucketName::new("test-bucket")?;
        let key = ObjectKey::new("nonexistent.txt")?;

        let view = RemoteView::new(pool, s3, bucket);
        let result = view.fetch_file_content(&key).await;
        assert!(result.is_err());
        Ok(())
    }
}
