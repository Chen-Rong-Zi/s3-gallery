//! Database models and CRUD operations for the s3-gallery core library.
//!
//! Each struct derives `sqlx::FromRow` and mirrors a table in the database
//! schema.  CRUD methods are implemented as `async fn` on each model.

use sqlx::SqlitePool;

use crate::error::Result;
use crate::error::S3GalleryError;

// ---------------------------------------------------------------------------
// HostConfigEntry
// ---------------------------------------------------------------------------

/// A row in the `host_config` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HostConfigEntry {
    /// Unique host identifier (primary key).
    pub host_id: String,
    /// Human-readable display name for the host.
    pub host_name: String,
    /// Type of host (e.g. "local", "s3", "gcs").
    pub host_type: String,
    /// Optional description of the host.
    pub description: String,
    /// ISO-8601 timestamp when the entry was created.
    pub created_at: String,
    /// The OSS bucket name this host was scanned from.
    pub bucket: String,
    /// The OSS endpoint URL this host was scanned from.
    pub endpoint: String,
    /// The OSS region this host was scanned from.
    pub region: String,
}

impl HostConfigEntry {
    /// Insert a new host config entry.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn insert(pool: &SqlitePool, entry: &HostConfigEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO host_config (host_id, host_name, host_type, description, created_at, bucket, endpoint, region) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.host_id)
        .bind(&entry.host_name)
        .bind(&entry.host_type)
        .bind(&entry.description)
        .bind(&entry.created_at)
        .bind(&entry.bucket)
        .bind(&entry.endpoint)
        .bind(&entry.region)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to insert host config: {e}")))?;
        Ok(())
    }

    /// Get a host config entry by its host id.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::NotFound` if no entry with the given id exists.
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn get(pool: &SqlitePool, host_id: &str) -> Result<HostConfigEntry> {
        sqlx::query_as::<_, HostConfigEntry>("SELECT * FROM host_config WHERE host_id = ?")
            .bind(host_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to get host config: {e}")))?
            .ok_or_else(|| S3GalleryError::NotFound(format!("Host config not found: {host_id}")))
    }

    /// List all host config entries, ordered by host_id.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<HostConfigEntry>> {
        sqlx::query_as::<_, HostConfigEntry>("SELECT * FROM host_config ORDER BY host_id")
            .fetch_all(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to list host configs: {e}")))
    }

    /// Update an existing host config entry.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn update(pool: &SqlitePool, entry: &HostConfigEntry) -> Result<()> {
        sqlx::query(
            "UPDATE host_config SET host_name = ?, host_type = ?, description = ?, created_at = ?, bucket = ?, endpoint = ?, region = ? \
             WHERE host_id = ?",
        )
        .bind(&entry.host_name)
        .bind(&entry.host_type)
        .bind(&entry.description)
        .bind(&entry.created_at)
        .bind(&entry.bucket)
        .bind(&entry.endpoint)
        .bind(&entry.region)
        .bind(&entry.host_id)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to update host config: {e}")))?;
        Ok(())
    }

    /// Insert or update the host config for a scan result.
    ///
    /// Creates a new row if the host_id doesn't exist, or updates the
    /// bucket, endpoint, and region fields if it does.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn upsert_host_config(
        pool: &SqlitePool,
        host_id: &str,
        bucket: &str,
        endpoint: &str,
        region: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO host_config (host_id, host_name, host_type, description, created_at, bucket, endpoint, region) \
             VALUES (?, 'unknown', 'unknown', '', datetime('now'), ?, ?, ?) \
             ON CONFLICT(host_id) DO UPDATE SET bucket = excluded.bucket, endpoint = excluded.endpoint, region = excluded.region"
        )
        .bind(host_id)
        .bind(bucket)
        .bind(endpoint)
        .bind(region)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to upsert host config: {e}")))?;
        Ok(())
    }

    /// Delete a host config entry by its host id.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn delete(pool: &SqlitePool, host_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM host_config WHERE host_id = ?")
            .bind(host_id)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to delete host config: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// FileEntry
// ---------------------------------------------------------------------------

/// A row in the `files` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FileEntry {
    /// Unique host identifier (part of composite primary key).
    pub host_id: String,
    /// Object key (part of composite primary key).
    pub key: String,
    /// HTTP entity tag.
    pub etag: String,
    /// File size in bytes.
    pub size: i64,
    /// ISO-8601 timestamp of the last modification time.
    pub last_modified: String,
    /// MIME content type (e.g. "image/jpeg").
    pub content_type: Option<String>,
    /// Canonical file type extension (e.g. "jpeg", "png").
    pub file_type: String,
    /// State of metadata extraction ("pending", "extracted", "failed").
    pub metadata_state: String,
    /// Effective date for timeline grouping (EXIF date or last_modified fallback).
    pub effective_date: String,
    /// Whether the file has been soft-deleted.
    pub is_deleted: bool,
}

impl FileEntry {
    /// Insert a new file entry.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn insert(pool: &SqlitePool, entry: &FileEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO files (host_id, key, etag, size, last_modified, content_type, file_type, \
             metadata_state, effective_date, is_deleted) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.host_id)
        .bind(&entry.key)
        .bind(&entry.etag)
        .bind(entry.size)
        .bind(&entry.last_modified)
        .bind(&entry.content_type)
        .bind(&entry.file_type)
        .bind(&entry.metadata_state)
        .bind(&entry.effective_date)
        .bind(entry.is_deleted)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to insert file: {e}")))?;
        Ok(())
    }

    /// Upsert a file entry (insert or replace).
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn upsert(pool: &SqlitePool, entry: &FileEntry) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO files (host_id, key, etag, size, last_modified, content_type, file_type, \
             metadata_state, effective_date, is_deleted) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.host_id)
        .bind(&entry.key)
        .bind(&entry.etag)
        .bind(entry.size)
        .bind(&entry.last_modified)
        .bind(&entry.content_type)
        .bind(&entry.file_type)
        .bind(&entry.metadata_state)
        .bind(&entry.effective_date)
        .bind(entry.is_deleted)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to upsert file: {e}")))?;
        Ok(())
    }

    /// Get a file entry by its host_id and key.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::NotFound` if no file with the given key exists.
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn get_by_key(pool: &SqlitePool, host_id: &str, key: &str) -> Result<FileEntry> {
        sqlx::query_as::<_, FileEntry>("SELECT * FROM files WHERE host_id = ? AND key = ?")
            .bind(host_id)
            .bind(key)
            .fetch_optional(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to get file: {e}")))?
            .ok_or_else(|| S3GalleryError::NotFound(format!("File not found: {key}")))
    }

    /// List files for a host whose key starts with the given prefix.
    ///
    /// Only non-deleted files are returned, ordered by key.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn list_by_prefix(
        pool: &SqlitePool,
        host_id: &str,
        prefix: &str,
    ) -> Result<Vec<FileEntry>> {
        sqlx::query_as::<_, FileEntry>(
            "SELECT * FROM files WHERE host_id = ? AND key LIKE ? || '%' AND is_deleted = 0 ORDER BY key",
        )
        .bind(host_id)
        .bind(prefix)
        .fetch_all(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to list files by prefix: {e}")))
    }

    /// List files for a host with the given file type.
    ///
    /// Only non-deleted files are returned, ordered by key.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn list_by_file_type(
        pool: &SqlitePool,
        host_id: &str,
        file_type: &str,
    ) -> Result<Vec<FileEntry>> {
        sqlx::query_as::<_, FileEntry>(
            "SELECT * FROM files WHERE host_id = ? AND file_type = ? AND is_deleted = 0 ORDER BY key",
        )
        .bind(host_id)
        .bind(file_type)
        .fetch_all(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to list files by type: {e}")))
    }

    /// Mark a file as deleted (soft delete).
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn mark_deleted(pool: &SqlitePool, host_id: &str, key: &str) -> Result<()> {
        sqlx::query("UPDATE files SET is_deleted = 1 WHERE host_id = ? AND key = ?")
            .bind(host_id)
            .bind(key)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to mark file deleted: {e}")))?;
        Ok(())
    }

    /// Count non-deleted files for a host.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn count(pool: &SqlitePool, host_id: &str) -> Result<i64> {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM files WHERE host_id = ? AND is_deleted = 0",
        )
        .bind(host_id)
        .fetch_one(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to count files: {e}")))
    }
}

// ---------------------------------------------------------------------------
// MetadataEntry
// ---------------------------------------------------------------------------

/// A row in the `metadata` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MetadataEntry {
    /// Foreign key referencing `files.key`.
    pub file_key: String,
    /// Metadata namespace (e.g. "exif", "video", "general").
    pub namespace: String,
    /// Metadata key within the namespace.
    pub key: String,
    /// Metadata value.
    pub value: String,
    /// ISO-8601 timestamp when the metadata was extracted.
    pub extracted_at: String,
    /// Whether the metadata is partial (incomplete extraction).
    pub partial: bool,
}

impl MetadataEntry {
    /// Insert a new metadata entry.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn insert(pool: &SqlitePool, entry: &MetadataEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO metadata (file_key, namespace, key, value, extracted_at, partial) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.file_key)
        .bind(&entry.namespace)
        .bind(&entry.key)
        .bind(&entry.value)
        .bind(&entry.extracted_at)
        .bind(entry.partial)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to insert metadata: {e}")))?;
        Ok(())
    }

    /// Get all metadata entries for a given file key.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn get_by_file_key(pool: &SqlitePool, file_key: &str) -> Result<Vec<MetadataEntry>> {
        sqlx::query_as::<_, MetadataEntry>(
            "SELECT * FROM metadata WHERE file_key = ? ORDER BY namespace, key",
        )
        .bind(file_key)
        .fetch_all(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to get metadata: {e}")))
    }

    /// Get all metadata entries for a given namespace.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn get_by_namespace(
        pool: &SqlitePool,
        namespace: &str,
    ) -> Result<Vec<MetadataEntry>> {
        sqlx::query_as::<_, MetadataEntry>(
            "SELECT * FROM metadata WHERE namespace = ? ORDER BY file_key, key",
        )
        .bind(namespace)
        .fetch_all(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to get metadata by namespace: {e}")))
    }

    /// Delete all metadata entries for a given file key.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn delete_by_file_key(pool: &SqlitePool, file_key: &str) -> Result<()> {
        sqlx::query("DELETE FROM metadata WHERE file_key = ?")
            .bind(file_key)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to delete metadata: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ThumbnailEntry
// ---------------------------------------------------------------------------

/// A row in the `thumbnails` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ThumbnailEntry {
    /// Foreign key referencing `files.key` (primary key).
    pub file_key: String,
    /// Binary thumbnail image data.
    pub data: Vec<u8>,
    /// Image format (e.g. "jpeg", "png").
    pub format: String,
    /// Thumbnail width in pixels, if known.
    pub width: Option<i64>,
    /// Thumbnail height in pixels, if known.
    pub height: Option<i64>,
    /// ISO-8601 timestamp when the thumbnail was cached.
    pub cached_at: String,
}

impl ThumbnailEntry {
    /// Insert a new thumbnail entry.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn insert(pool: &SqlitePool, entry: &ThumbnailEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO thumbnails (file_key, data, format, width, height, cached_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.file_key)
        .bind(&entry.data)
        .bind(&entry.format)
        .bind(entry.width)
        .bind(entry.height)
        .bind(&entry.cached_at)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to insert thumbnail: {e}")))?;
        Ok(())
    }

    /// Get a thumbnail entry by its file key.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::NotFound` if no thumbnail with the given key exists.
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn get(pool: &SqlitePool, file_key: &str) -> Result<ThumbnailEntry> {
        sqlx::query_as::<_, ThumbnailEntry>("SELECT * FROM thumbnails WHERE file_key = ?")
            .bind(file_key)
            .fetch_optional(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to get thumbnail: {e}")))?
            .ok_or_else(|| S3GalleryError::NotFound(format!("Thumbnail not found: {file_key}")))
    }

    /// Update an existing thumbnail entry.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn update(pool: &SqlitePool, entry: &ThumbnailEntry) -> Result<()> {
        sqlx::query(
            "UPDATE thumbnails SET data = ?, format = ?, width = ?, height = ?, cached_at = ? \
             WHERE file_key = ?",
        )
        .bind(&entry.data)
        .bind(&entry.format)
        .bind(entry.width)
        .bind(entry.height)
        .bind(&entry.cached_at)
        .bind(&entry.file_key)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to update thumbnail: {e}")))?;
        Ok(())
    }

    /// Delete a thumbnail entry by its file key.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn delete(pool: &SqlitePool, file_key: &str) -> Result<()> {
        sqlx::query("DELETE FROM thumbnails WHERE file_key = ?")
            .bind(file_key)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to delete thumbnail: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TagEntry
// ---------------------------------------------------------------------------

/// A row in the `tags` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TagEntry {
    /// Auto-incrementing primary key.
    pub tag_id: i64,
    /// Unique tag name.
    pub tag_name: String,
    /// Type of tag (e.g. "auto", "manual").
    pub tag_type: String,
}

impl TagEntry {
    /// Insert a new tag entry.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn insert(pool: &SqlitePool, entry: &TagEntry) -> Result<()> {
        sqlx::query("INSERT INTO tags (tag_name, tag_type) VALUES (?, ?)")
            .bind(&entry.tag_name)
            .bind(&entry.tag_type)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to insert tag: {e}")))?;
        Ok(())
    }

    /// Get a tag entry by its name.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::NotFound` if no tag with the given name exists.
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn get_by_name(pool: &SqlitePool, tag_name: &str) -> Result<TagEntry> {
        sqlx::query_as::<_, TagEntry>("SELECT * FROM tags WHERE tag_name = ?")
            .bind(tag_name)
            .fetch_optional(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to get tag: {e}")))?
            .ok_or_else(|| S3GalleryError::NotFound(format!("Tag not found: {tag_name}")))
    }

    /// List all tags, ordered by tag name.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<TagEntry>> {
        sqlx::query_as::<_, TagEntry>("SELECT * FROM tags ORDER BY tag_name")
            .fetch_all(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to list tags: {e}")))
    }

    /// Ensure a tag exists, creating it if necessary. Returns the tag_id.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn ensure_exists(pool: &SqlitePool, tag_name: &str, tag_type: &str) -> Result<i64> {
        sqlx::query("INSERT OR IGNORE INTO tags (tag_name, tag_type) VALUES (?, ?)")
            .bind(tag_name)
            .bind(tag_type)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        let row: (i64,) = sqlx::query_as("SELECT tag_id FROM tags WHERE tag_name = ?")
            .bind(tag_name)
            .fetch_one(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        Ok(row.0)
    }
}

// ---------------------------------------------------------------------------
// FileTagEntry
// ---------------------------------------------------------------------------

/// A row in the `file_tags` join table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FileTagEntry {
    /// Foreign key referencing `files.key`.
    pub file_key: String,
    /// Foreign key referencing `tags.tag_id`.
    pub tag_id: i64,
}

impl FileTagEntry {
    /// Insert a new file-tag association.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn insert(pool: &SqlitePool, entry: &FileTagEntry) -> Result<()> {
        sqlx::query("INSERT INTO file_tags (file_key, tag_id) VALUES (?, ?)")
            .bind(&entry.file_key)
            .bind(entry.tag_id)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to insert file tag: {e}")))?;
        Ok(())
    }

    /// Get all tag associations for a given file key.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn get_by_file_key(pool: &SqlitePool, file_key: &str) -> Result<Vec<FileTagEntry>> {
        sqlx::query_as::<_, FileTagEntry>("SELECT * FROM file_tags WHERE file_key = ?")
            .bind(file_key)
            .fetch_all(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to get file tags: {e}")))
    }

    /// Delete a specific file-tag association.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn delete(pool: &SqlitePool, file_key: &str, tag_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM file_tags WHERE file_key = ? AND tag_id = ?")
            .bind(file_key)
            .bind(tag_id)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to delete file tag: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ScanMetadata
// ---------------------------------------------------------------------------

/// A row in the `scan_metadata` table (one row per host).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScanMetadata {
    /// Unique host identifier (primary key).
    pub host_id: String,
    /// The key of the last scanned file (for incremental scans).
    pub last_scanned_key: Option<String>,
    /// ISO-8601 timestamp of the last scan.
    pub last_scanned_at: Option<String>,
    /// Total number of files at last scan.
    pub total_files: Option<i64>,
    /// Total size of all files in bytes at last scan.
    pub total_size: Option<i64>,
    /// Database schema version number.
    pub db_schema_version: i64,
}

impl ScanMetadata {
    /// Get the scan metadata row for a given host.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::NotFound` if no scan metadata row exists for the host.
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn get(pool: &SqlitePool, host_id: &str) -> Result<ScanMetadata> {
        sqlx::query_as::<_, ScanMetadata>("SELECT * FROM scan_metadata WHERE host_id = ? LIMIT 1")
            .bind(host_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to get scan metadata: {e}")))?
            .ok_or_else(|| {
                S3GalleryError::NotFound(format!("Scan metadata not found for host: {host_id}"))
            })
    }

    /// Update the scan metadata row for a host (inserts if not exists).
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::DbError` if the database operation fails.
    pub async fn update(pool: &SqlitePool, metadata: &ScanMetadata) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO scan_metadata (host_id, last_scanned_key, last_scanned_at, \
             total_files, total_size, db_schema_version) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&metadata.host_id)
        .bind(&metadata.last_scanned_key)
        .bind(&metadata.last_scanned_at)
        .bind(metadata.total_files)
        .bind(metadata.total_size)
        .bind(metadata.db_schema_version)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to update scan metadata: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DirSizeEntry
// ---------------------------------------------------------------------------

/// A row in the `dir_sizes` table, storing aggregate size for a directory.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DirSizeEntry {
    /// Host that owns this directory.
    pub host_id: String,
    /// Directory path (always ends with '/').
    pub dir_path: String,
    /// Total size of all files under this directory.
    pub total_size: i64,
    /// Total number of files under this directory.
    pub total_files: i64,
}

impl DirSizeEntry {
    /// Get the size of a specific directory.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::NotFound` if no entry exists.
    pub async fn get(pool: &SqlitePool, host_id: &str, dir_path: &str) -> Result<DirSizeEntry> {
        sqlx::query_as::<_, DirSizeEntry>(
            "SELECT * FROM dir_sizes WHERE host_id = ? AND dir_path = ?"
        )
        .bind(host_id)
        .bind(dir_path)
        .fetch_optional(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to get dir size: {e}")))?
        .ok_or_else(|| {
            S3GalleryError::NotFound(format!("Dir size not found: {host_id}/{dir_path}"))
        })
    }

    /// List all subdirectory sizes under a given prefix.
    /// Excludes the exact prefix itself.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn list_by_prefix(
        pool: &SqlitePool,
        host_id: &str,
        prefix: &str,
    ) -> Result<Vec<DirSizeEntry>> {
        sqlx::query_as::<_, DirSizeEntry>(
            "SELECT * FROM dir_sizes \
             WHERE host_id = ? AND dir_path != ? AND dir_path LIKE ? \
             ORDER BY dir_path"
        )
        .bind(host_id)
        .bind(prefix)
        .bind(format!("{}%", prefix))
        .fetch_all(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to list dir sizes: {e}")))
    }

    /// Upsert a directory size entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn upsert(
        pool: &SqlitePool,
        host_id: &str,
        dir_path: &str,
        total_size: i64,
        total_files: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO dir_sizes (host_id, dir_path, total_size, total_files) \
             VALUES (?, ?, ?, ?)"
        )
        .bind(host_id)
        .bind(dir_path)
        .bind(total_size)
        .bind(total_files)
        .execute(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to upsert dir size: {e}")))?;
        Ok(())
    }

    /// Delete all dir_sizes entries for a given host.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn delete_by_host(pool: &SqlitePool, host_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM dir_sizes WHERE host_id = ?")
            .bind(host_id)
            .execute(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to delete dir sizes: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    /// Create a temporary in-memory database with migrations applied.
    async fn setup_test_db() -> Result<(SqlitePool, tempfile::TempDir)> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;
        Ok((pool, dir))
    }

    // -- HostConfigEntry tests -----------------------------------------------

    #[tokio::test]
    async fn test_host_config_crud() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        let entry = HostConfigEntry {
            host_id: "host-01".to_string(),
            host_name: "Primary Host".to_string(),
            host_type: "local".to_string(),
            description: "Main storage host".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            bucket: "".to_string(),
            endpoint: "".to_string(),
            region: "".to_string(),
        };

        // Insert
        HostConfigEntry::insert(&pool, &entry).await?;

        // Read back
        let fetched = HostConfigEntry::get(&pool, "host-01").await?;
        assert_eq!(fetched.host_name, "Primary Host");
        assert_eq!(fetched.host_type, "local");

        // Update
        let updated = HostConfigEntry {
            description: "Updated description".to_string(),
            ..entry
        };
        HostConfigEntry::update(&pool, &updated).await?;
        let fetched = HostConfigEntry::get(&pool, "host-01").await?;
        assert_eq!(fetched.description, "Updated description");

        // Delete
        HostConfigEntry::delete(&pool, "host-01").await?;
        let result = HostConfigEntry::get(&pool, "host-01").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(S3GalleryError::NotFound(_))));

        Ok(())
    }

    #[tokio::test]
    async fn test_host_config_get_not_found() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        let result = HostConfigEntry::get(&pool, "nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(S3GalleryError::NotFound(_))));
        Ok(())
    }

    #[tokio::test]
    async fn test_host_config_upsert_host_config() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        // INSERT path: new host_id creates a row
        HostConfigEntry::upsert_host_config(&pool, "camera-1", "photos-bucket", "https://oss.example.com", "us-east-1").await?;
        let config = HostConfigEntry::get(&pool, "camera-1").await?;
        assert_eq!(config.bucket, "photos-bucket");
        assert_eq!(config.endpoint, "https://oss.example.com");
        assert_eq!(config.region, "us-east-1");

        // UPDATE path: existing host_id updates bucket, endpoint, region
        HostConfigEntry::upsert_host_config(&pool, "camera-1", "new-bucket", "https://oss2.example.com", "eu-west-1").await?;
        let config = HostConfigEntry::get(&pool, "camera-1").await?;
        assert_eq!(config.bucket, "new-bucket");
        assert_eq!(config.endpoint, "https://oss2.example.com");
        assert_eq!(config.region, "eu-west-1");

        Ok(())
    }

    #[tokio::test]
    async fn test_host_config_list_all() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        // Empty database returns empty list
        let hosts = HostConfigEntry::list_all(&pool).await?;
        assert!(hosts.is_empty());

        // Insert two hosts
        HostConfigEntry::upsert_host_config(&pool, "host-a", "bucket-a", "https://endpoint-a", "us-east-1").await?;
        HostConfigEntry::upsert_host_config(&pool, "host-b", "bucket-b", "https://endpoint-b", "eu-west-1").await?;

        let hosts = HostConfigEntry::list_all(&pool).await?;
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].host_id, "host-a");
        assert_eq!(hosts[1].host_id, "host-b");

        Ok(())
    }

    // -- FileEntry tests -----------------------------------------------------

    #[tokio::test]
    async fn test_file_entry_crud() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        let entry = FileEntry {
            host_id: "test-host".to_string(),
            key: "test/file.jpg".to_string(),
            etag: "\"abc123\"".to_string(),
            size: 1024,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        };

        // Insert
        FileEntry::insert(&pool, &entry).await?;

        // Read back
        let fetched = FileEntry::get_by_key(&pool, "test-host", "test/file.jpg").await?;
        assert_eq!(fetched.etag, "\"abc123\"");
        assert_eq!(fetched.size, 1024);
        assert_eq!(fetched.content_type, Some("image/jpeg".to_string()));

        // Upsert (update etag)
        let upserted = FileEntry {
            host_id: "test-host".to_string(),
            etag: "\"def456\"".to_string(),
            size: 2048,
            ..entry
        };
        FileEntry::upsert(&pool, &upserted).await?;
        let fetched = FileEntry::get_by_key(&pool, "test-host", "test/file.jpg").await?;
        assert_eq!(fetched.etag, "\"def456\"");
        assert_eq!(fetched.size, 2048);

        // Mark as deleted
        FileEntry::mark_deleted(&pool, "test-host", "test/file.jpg").await?;
        let fetched = FileEntry::get_by_key(&pool, "test-host", "test/file.jpg").await?;
        assert!(fetched.is_deleted);

        Ok(())
    }

    #[tokio::test]
    async fn test_file_entry_get_not_found() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        let result = FileEntry::get_by_key(&pool, "test-host", "nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(S3GalleryError::NotFound(_))));
        Ok(())
    }

    #[tokio::test]
    async fn test_file_entry_list_by_prefix() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        let entries = &[
            FileEntry {
                host_id: "test-host".to_string(),
                key: "photos/vacation/img001.jpg".to_string(),
                etag: "\"a\"".to_string(),
                size: 100,
                last_modified: "2026-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
            FileEntry {
                host_id: "test-host".to_string(),
                key: "photos/vacation/img002.jpg".to_string(),
                etag: "\"b\"".to_string(),
                size: 200,
                last_modified: "2026-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
            FileEntry {
                host_id: "test-host".to_string(),
                key: "docs/report.pdf".to_string(),
                etag: "\"c\"".to_string(),
                size: 300,
                last_modified: "2026-01-01T00:00:00Z".to_string(),
                content_type: Some("application/pdf".to_string()),
                file_type: "pdf".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        ];

        for entry in entries {
            FileEntry::insert(&pool, entry).await?;
        }

        // List by prefix "photos/"
        let results = FileEntry::list_by_prefix(&pool, "test-host", "photos/").await?;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].key, "photos/vacation/img001.jpg");
        assert_eq!(results[1].key, "photos/vacation/img002.jpg");

        // List by prefix "docs/"
        let results = FileEntry::list_by_prefix(&pool, "test-host", "docs/").await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "docs/report.pdf");

        // List by prefix with no matches
        let results = FileEntry::list_by_prefix(&pool, "test-host", "nonexistent/").await?;
        assert!(results.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_file_entry_list_by_file_type() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        let entries = &[
            FileEntry {
                host_id: "test-host".to_string(),
                key: "img001.jpg".to_string(),
                etag: "\"a\"".to_string(),
                size: 100,
                last_modified: "2026-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
            FileEntry {
                host_id: "test-host".to_string(),
                key: "doc.pdf".to_string(),
                etag: "\"b\"".to_string(),
                size: 200,
                last_modified: "2026-01-01T00:00:00Z".to_string(),
                content_type: Some("application/pdf".to_string()),
                file_type: "pdf".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        ];

        for entry in entries {
            FileEntry::insert(&pool, entry).await?;
        }

        let results = FileEntry::list_by_file_type(&pool, "test-host", "jpeg").await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "img001.jpg");

        let results = FileEntry::list_by_file_type(&pool, "test-host", "pdf").await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "doc.pdf");

        let results = FileEntry::list_by_file_type(&pool, "test-host", "png").await?;
        assert!(results.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_file_entry_count() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        let count = FileEntry::count(&pool, "test-host").await?;
        assert_eq!(count, 0);

        let entry = FileEntry {
            host_id: "test-host".to_string(),
            key: "test.txt".to_string(),
            etag: "\"a\"".to_string(),
            size: 100,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: None,
            file_type: "txt".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        };
        FileEntry::insert(&pool, &entry).await?;

        let count = FileEntry::count(&pool, "test-host").await?;
        assert_eq!(count, 1);

        // Deleted files are not counted
        FileEntry::mark_deleted(&pool, "test-host", "test.txt").await?;
        let count = FileEntry::count(&pool, "test-host").await?;
        assert_eq!(count, 0);

        Ok(())
    }

    // -- MetadataEntry tests -------------------------------------------------

    #[tokio::test]
    async fn test_metadata_entry_crud() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        // Insert a file first (foreign key constraint)
        let file = FileEntry {
            host_id: "test-host".to_string(),
            key: "test/file.jpg".to_string(),
            etag: "\"a\"".to_string(),
            size: 100,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        };
        FileEntry::insert(&pool, &file).await?;

        let meta = MetadataEntry {
            file_key: "test/file.jpg".to_string(),
            namespace: "exif".to_string(),
            key: "Make".to_string(),
            value: "Canon".to_string(),
            extracted_at: "2026-01-01T00:00:00Z".to_string(),
            partial: false,
        };
        MetadataEntry::insert(&pool, &meta).await?;

        // Read back by file key
        let results = MetadataEntry::get_by_file_key(&pool, "test/file.jpg").await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "Make");
        assert_eq!(results[0].value, "Canon");

        // Read back by namespace
        let results = MetadataEntry::get_by_namespace(&pool, "exif").await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_key, "test/file.jpg");

        // Delete
        MetadataEntry::delete_by_file_key(&pool, "test/file.jpg").await?;
        let results = MetadataEntry::get_by_file_key(&pool, "test/file.jpg").await?;
        assert!(results.is_empty());

        Ok(())
    }

    // -- ThumbnailEntry tests ------------------------------------------------

    #[tokio::test]
    async fn test_thumbnail_crud() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        // Insert a file first (foreign key constraint)
        let file = FileEntry {
            host_id: "test-host".to_string(),
            key: "test/file.jpg".to_string(),
            etag: "\"a\"".to_string(),
            size: 100,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        };
        FileEntry::insert(&pool, &file).await?;

        let thumb = ThumbnailEntry {
            file_key: "test/file.jpg".to_string(),
            data: vec![0xFF, 0xD8, 0xFF, 0xE0],
            format: "jpeg".to_string(),
            width: Some(100),
            height: Some(100),
            cached_at: "2026-01-01T00:00:00Z".to_string(),
        };
        ThumbnailEntry::insert(&pool, &thumb).await?;

        // Read back
        let fetched = ThumbnailEntry::get(&pool, "test/file.jpg").await?;
        assert_eq!(fetched.format, "jpeg");
        assert_eq!(fetched.width, Some(100));
        assert_eq!(fetched.height, Some(100));

        // Update
        let updated = ThumbnailEntry {
            width: Some(200),
            height: Some(200),
            ..thumb
        };
        ThumbnailEntry::update(&pool, &updated).await?;
        let fetched = ThumbnailEntry::get(&pool, "test/file.jpg").await?;
        assert_eq!(fetched.width, Some(200));
        assert_eq!(fetched.height, Some(200));

        // Delete
        ThumbnailEntry::delete(&pool, "test/file.jpg").await?;
        let result = ThumbnailEntry::get(&pool, "test/file.jpg").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(S3GalleryError::NotFound(_))));

        Ok(())
    }

    #[tokio::test]
    async fn test_thumbnail_get_not_found() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        let result = ThumbnailEntry::get(&pool, "nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(S3GalleryError::NotFound(_))));
        Ok(())
    }

    // -- TagEntry tests ------------------------------------------------------

    #[tokio::test]
    async fn test_tag_entry_crud() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        let tag = TagEntry {
            tag_id: 0, // SQLite AUTOINCREMENT will assign this
            tag_name: "landscape".to_string(),
            tag_type: "auto".to_string(),
        };
        TagEntry::insert(&pool, &tag).await?;

        // Read back by name
        let fetched = TagEntry::get_by_name(&pool, "landscape").await?;
        assert_eq!(fetched.tag_name, "landscape");
        assert_eq!(fetched.tag_type, "auto");
        assert!(fetched.tag_id > 0);

        // List all
        let tags = TagEntry::list_all(&pool).await?;
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].tag_name, "landscape");

        // Get by name not found
        let result = TagEntry::get_by_name(&pool, "nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(S3GalleryError::NotFound(_))));

        Ok(())
    }

    // -- FileTagEntry tests --------------------------------------------------

    #[tokio::test]
    async fn test_file_tag_crud() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        // Insert a file
        let file = FileEntry {
            host_id: "test-host".to_string(),
            key: "test/file.jpg".to_string(),
            etag: "\"a\"".to_string(),
            size: 100,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        };
        FileEntry::insert(&pool, &file).await?;

        // Insert a tag
        let tag = TagEntry {
            tag_id: 0,
            tag_name: "landscape".to_string(),
            tag_type: "auto".to_string(),
        };
        TagEntry::insert(&pool, &tag).await?;
        let tag = TagEntry::get_by_name(&pool, "landscape").await?;

        // Insert file-tag association
        let ft = FileTagEntry {
            file_key: "test/file.jpg".to_string(),
            tag_id: tag.tag_id,
        };
        FileTagEntry::insert(&pool, &ft).await?;

        // Read back by file key
        let results = FileTagEntry::get_by_file_key(&pool, "test/file.jpg").await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tag_id, tag.tag_id);

        // Delete
        FileTagEntry::delete(&pool, "test/file.jpg", tag.tag_id).await?;
        let results = FileTagEntry::get_by_file_key(&pool, "test/file.jpg").await?;
        assert!(results.is_empty());

        Ok(())
    }

    // -- ScanMetadata tests --------------------------------------------------

    #[tokio::test]
    async fn test_scan_metadata_crud() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;

        // The row should exist after migration with default schema version for host 'default'
        let fetched = ScanMetadata::get(&pool, "default").await?;
        assert_eq!(fetched.db_schema_version, 1);
        assert!(fetched.last_scanned_key.is_none());

        // Update
        let updated = ScanMetadata {
            host_id: "default".to_string(),
            last_scanned_key: Some("last/file.txt".to_string()),
            last_scanned_at: Some("2026-06-01T00:00:00Z".to_string()),
            total_files: Some(100),
            total_size: Some(1048576),
            db_schema_version: 1,
        };
        ScanMetadata::update(&pool, &updated).await?;

        let fetched = ScanMetadata::get(&pool, "default").await?;
        assert_eq!(fetched.last_scanned_key, Some("last/file.txt".to_string()));
        assert_eq!(fetched.total_files, Some(100));
        assert_eq!(fetched.total_size, Some(1048576));

        Ok(())
    }
}
