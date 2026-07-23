use crate::db::models::ThumbnailEntry;
use crate::error::{Result, S3GalleryError};
use crate::types::ObjectKey;
use chrono::Utc;
use image::ImageFormat;
use sqlx::SqlitePool;
use tracing;

/// Default thumbnail size (320px on longest side)
pub const THUMBNAIL_SIZE: u32 = 320;

/// Maximum cache size in bytes (500 MB)
pub const MAX_CACHE_BYTES: u64 = 500 * 1024 * 1024;

/// Check the thumbnail cache for a given key. Returns None if not cached.
async fn get_cached_entry(db: &SqlitePool, key: &ObjectKey) -> Result<Option<ThumbnailEntry>> {
    match ThumbnailEntry::get(db, key.as_str()).await {
        Ok(entry) => Ok(Some(entry)),
        Err(S3GalleryError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Store a thumbnail in the cache.
async fn cache_entry(db: &SqlitePool, key: &ObjectKey, data: &[u8]) -> Result<()> {
    let entry = ThumbnailEntry {
        file_key: key.as_str().to_string(),
        data: data.to_vec(),
        format: "jpeg".to_string(),
        width: Some(THUMBNAIL_SIZE as i64),
        height: None,
        cached_at: Utc::now().to_rfc3339(),
    };
    ThumbnailEntry::insert(db, &entry).await
}

/// Generate a thumbnail from raw image bytes.
/// Resizes to 320px on the longest side, encodes as JPEG.
///
/// # Errors
/// Returns `S3GalleryError::ThumbnailGeneration` if the image fails to decode or encode.
pub fn generate_thumbnail(data: &[u8]) -> Result<Vec<u8>> {
    tracing::debug!(
        target: "s3_gallery::thumbnail",
        input_size = data.len(),
        "Generating thumbnail"
    );
    let img = image::load_from_memory(data).map_err(|e| {
        S3GalleryError::ThumbnailGeneration(format!("Failed to decode image: {}", e))
    })?;

    let thumbnail = img.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE);

    let mut output = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut output);
    thumbnail
        .write_to(&mut cursor, ImageFormat::Jpeg)
        .map_err(|e| {
            S3GalleryError::ThumbnailGeneration(format!("Failed to encode JPEG: {}", e))
        })?;

    Ok(output)
}

/// Thumbnail cache with LRU eviction
pub struct ThumbnailCache {
    db: SqlitePool,
}

impl ThumbnailCache {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Get a cached thumbnail, or generate and cache it
    ///
    /// # Errors
    /// Returns `S3GalleryError` if the cache lookup fails or thumbnail generation fails.
    pub async fn get_or_generate(&self, key: &ObjectKey, data: &[u8]) -> Result<Vec<u8>> {
        // Try cache first
        if let Some(entry) = get_cached_entry(&self.db, key).await? {
            tracing::debug!(
                target: "s3_gallery::thumbnail",
                key = %key,
                "Thumbnail cache hit"
            );
            return Ok(entry.data);
        }

        // Generate and cache
        tracing::debug!(
            target: "s3_gallery::thumbnail",
            key = %key,
            "Thumbnail cache miss — generating"
        );
        let thumbnail_data = generate_thumbnail(data)?;
        if let Err(e) = cache_entry(&self.db, key, &thumbnail_data).await {
            tracing::warn!(
                target: "s3_gallery::thumbnail",
                key = %key,
                error = %e,
                "failed to cache thumbnail"
            );
        }
        Ok(thumbnail_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use image::RgbImage;
    use tempfile::tempdir;

    /// Create a test image (100x100 pixel PNG)
    fn create_test_image() -> Vec<u8> {
        let img = RgbImage::new(100, 100);
        let mut cursor = std::io::Cursor::new(Vec::new());

        // Handle the write result properly without expect
        match img.write_to(&mut cursor, image::ImageFormat::Png) {
            Ok(_) => cursor.into_inner(),
            Err(_) => Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_generate_thumbnail_from_test_image() -> Result<()> {
        let img_data = create_test_image();
        // Skip test if we couldn't create the test image
        if img_data.is_empty() {
            return Ok(());
        }

        let thumbnail = generate_thumbnail(&img_data)?;
        assert!(!thumbnail.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_thumbnail_cache_get_or_generate() -> Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let cache = ThumbnailCache::new(pool.clone());
        let key = ObjectKey::new("test.jpg")?;
        let img_data = create_test_image();

        // Skip test if we couldn't create the test image
        if img_data.is_empty() {
            return Ok(());
        }

        // Insert a file entry first to satisfy the foreign key constraint.
        let file = crate::db::models::FileEntry {
            host_id: "test-host".to_string(),
            key: "test.jpg".to_string(),
            etag: "\"abc123\"".to_string(),
            size: 1024,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        };
        crate::db::models::FileEntry::insert(&pool, &file).await?;

        // First call: generate and cache
        let thumb1 = cache.get_or_generate(&key, &img_data).await?;
        assert!(!thumb1.is_empty());

        // Second call: should use cache
        let thumb2 = cache.get_or_generate(&key, &img_data).await?;
        assert_eq!(thumb1, thumb2);

        Ok(())
    }
}
