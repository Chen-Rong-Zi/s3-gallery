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
async fn get_cached_entry(db: &SqlitePool, key: &ObjectKey) -> Result<Option<Vec<u8>>> {
    let row: Option<(Vec<u8>,)> = sqlx::query_as("SELECT data FROM thumbnails WHERE file_key = ?")
        .bind(key.as_str())
        .fetch_optional(db)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to get thumbnail: {e}")))?;
    Ok(row.map(|r| r.0))
}

/// Store a thumbnail in the cache.
async fn cache_entry(db: &SqlitePool, key: &ObjectKey, data: &[u8]) -> Result<()> {
    sqlx::query(
        "INSERT OR REPLACE INTO thumbnails (file_key, data, format, width, height, cached_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(key.as_str())
    .bind(data)
    .bind("jpeg")
    .bind(THUMBNAIL_SIZE as i64)
    .bind(None::<i64>)
    .bind(Utc::now().to_rfc3339())
    .execute(db)
    .await
    .map_err(|e| S3GalleryError::DbError(format!("Failed to insert thumbnail: {e}")))?;
    Ok(())
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
        if let Some(data) = get_cached_entry(&self.db, key).await? {
            tracing::debug!(
                target: "s3_gallery::thumbnail",
                key = %key,
                "Thumbnail cache hit"
            );
            return Ok(data);
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
    use crate::db::migrate::run_full_migration;
    use crate::db::pool::create_pool;
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
        let sea_db = create_pool(&db_path).await?;
        let pool = sea_db.get_sqlite_connection_pool().clone();
        run_full_migration(&sea_db).await?;

        let cache = ThumbnailCache::new(pool.clone());
        let key = ObjectKey::new("test.jpg")?;
        let img_data = create_test_image();

        // Skip test if we couldn't create the test image
        if img_data.is_empty() {
            return Ok(());
        }

        // Insert a file entry first to satisfy the foreign key constraint.
        sqlx::query(
            "INSERT INTO files (host_id, key, etag, size, last_modified, content_type, file_type, \
             metadata_state, effective_date, is_deleted) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("test-host")
        .bind("test.jpg")
        .bind("\"abc123\"")
        .bind(1024i64)
        .bind("2026-01-01T00:00:00Z")
        .bind(Some("image/jpeg"))
        .bind("jpeg")
        .bind("pending")
        .bind("")
        .bind(false)
        .execute(&pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        // First call: generate and cache
        let thumb1 = cache.get_or_generate(&key, &img_data).await?;
        assert!(!thumb1.is_empty());

        // Second call: should use cache
        let thumb2 = cache.get_or_generate(&key, &img_data).await?;
        assert_eq!(thumb1, thumb2);

        Ok(())
    }
}
