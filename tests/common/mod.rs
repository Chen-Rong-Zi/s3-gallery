//! Shared test setup helpers for integration tests.
//!
//! Provides reusable functions to create temporary databases, seed test data,
//! create mock S3 clients, and build valid configs.

// Functions in this module are used by different test binaries; each binary
// only uses a subset, so suppress dead_code warnings.
#![allow(dead_code)]

use std::sync::Arc;

use s3_gallery_core::db::models::{
    FileEntry, FileTagEntry, MetadataEntry, TagEntry, ThumbnailEntry,
};
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::mock::MockS3Client;
use s3_gallery_core::types::BucketName;
use sea_orm::DatabaseConnection;
use sqlx::SqlitePool;
use tempfile::TempDir;

/// Create a temporary SQLite database, run migrations, and return the connection
/// along with the temp directory handle (kept alive for the lifetime of the
/// returned `TempDir`).
pub async fn setup_test_db() -> Result<(DatabaseConnection, TempDir)> {
    let dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let db_path = dir.path().join("test.db");
    let db = create_pool(&db_path).await?;
    run_migrations(db.get_sqlite_connection_pool()).await?;
    Ok((db, dir))
}

/// Seed the database with a known set of test files.
///
/// Returns the number of files inserted.
pub async fn seed_test_files(pool: &sqlx::SqlitePool) -> Result<usize> {
    let files = vec![
        FileEntry {
            host_id: "test-host".to_string(),
            key: "photos/vacation/img001.jpg".to_string(),
            etag: "etag-001".to_string(),
            size: 102400,
            last_modified: "2026-06-01T12:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "extracted".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
        FileEntry {
            host_id: "test-host".to_string(),
            key: "photos/vacation/img002.jpg".to_string(),
            etag: "etag-002".to_string(),
            size: 204800,
            last_modified: "2026-06-02T12:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
        FileEntry {
            host_id: "test-host".to_string(),
            key: "photos/party/clip001.mp4".to_string(),
            etag: "etag-003".to_string(),
            size: 5242880,
            last_modified: "2026-06-03T12:00:00Z".to_string(),
            content_type: Some("video/mp4".to_string()),
            file_type: "mp4".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
        FileEntry {
            host_id: "test-host".to_string(),
            key: "docs/report.pdf".to_string(),
            etag: "etag-004".to_string(),
            size: 307200,
            last_modified: "2026-06-04T12:00:00Z".to_string(),
            content_type: Some("application/pdf".to_string()),
            file_type: "pdf".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
        FileEntry {
            host_id: "test-host".to_string(),
            key: "docs/notes.txt".to_string(),
            etag: "etag-005".to_string(),
            size: 5120,
            last_modified: "2026-06-05T12:00:00Z".to_string(),
            content_type: Some("text/plain".to_string()),
            file_type: "txt".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
    ];

    let count = files.len();
    for file in &files {
        FileEntry::insert(pool, file).await?;
    }
    Ok(count)
}

/// Seed the database with tags and file-tag associations.
pub async fn seed_test_tags(pool: &SqlitePool) -> Result<()> {
    let tag = TagEntry {
        tag_id: 0,
        tag_name: "vacation".to_string(),
        tag_type: "manual".to_string(),
    };
    TagEntry::insert(pool, &tag).await?;
    let tag = TagEntry::get_by_name(pool, "vacation").await?;

    let ft = FileTagEntry {
        file_key: "photos/vacation/img001.jpg".to_string(),
        tag_id: tag.tag_id,
    };
    FileTagEntry::insert(pool, &ft).await?;

    let ft = FileTagEntry {
        file_key: "photos/vacation/img002.jpg".to_string(),
        tag_id: tag.tag_id,
    };
    FileTagEntry::insert(pool, &ft).await?;

    Ok(())
}

/// Seed the database with metadata entries.
pub async fn seed_test_metadata(pool: &SqlitePool) -> Result<()> {
    let meta = MetadataEntry {
        file_key: "photos/vacation/img001.jpg".to_string(),
        namespace: "exif".to_string(),
        key: "Make".to_string(),
        value: "Canon".to_string(),
        extracted_at: "2026-06-01T12:00:00Z".to_string(),
        partial: false,
    };
    MetadataEntry::insert(pool, &meta).await?;

    let meta = MetadataEntry {
        file_key: "photos/vacation/img001.jpg".to_string(),
        namespace: "exif".to_string(),
        key: "Model".to_string(),
        value: "EOS R5".to_string(),
        extracted_at: "2026-06-01T12:00:00Z".to_string(),
        partial: false,
    };
    MetadataEntry::insert(pool, &meta).await?;

    Ok(())
}

/// Seed the database with a thumbnail entry.
pub async fn seed_test_thumbnail(pool: &SqlitePool) -> Result<()> {
    let thumb = ThumbnailEntry {
        file_key: "photos/vacation/img001.jpg".to_string(),
        data: vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10],
        format: "jpeg".to_string(),
        width: Some(150),
        height: Some(150),
        cached_at: "2026-06-01T12:00:00Z".to_string(),
    };
    ThumbnailEntry::insert(pool, &thumb).await?;
    Ok(())
}

/// Create a `MockS3Client` pre-populated with test objects.
///
/// Objects include a mix of images, videos, and documents.
pub fn create_mock_s3() -> Result<MockS3Client> {
    MockS3Client::with_fixtures(vec![
        ("photos/vacation/img001.jpg", b"fake jpeg data for img001"),
        ("photos/vacation/img002.jpg", b"fake jpeg data for img002"),
        ("photos/party/clip001.mp4", b"fake mp4 data for clip001"),
        ("docs/report.pdf", b"fake pdf content"),
        ("docs/notes.txt", b"some text notes"),
    ])
}

/// Create a mock `Arc<dyn S3Client>` with test objects.
pub fn create_mock_s3_arc() -> Result<Arc<dyn S3Client>> {
    let mock = create_mock_s3()?;
    Ok(Arc::new(mock) as Arc<dyn S3Client>)
}

/// Create a valid `OssConfig` for testing.
pub fn create_test_config() -> Result<OssConfig> {
    let bucket = BucketName::new("test-bucket")?;
    OssConfig::validate(
        bucket,
        "http://localhost:9000",
        "us-east-1",
        "test-access-key",
        "test-secret-key",
        10,
    )
}

/// A test-friendly bucket name constant.
pub fn test_bucket() -> Result<BucketName> {
    BucketName::new("test-bucket")
}
