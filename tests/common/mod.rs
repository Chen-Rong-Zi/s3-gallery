//! Shared test setup helpers for integration tests.
//!
//! Provides reusable functions to create temporary databases, seed test data,
//! create mock S3 clients, and build valid configs.

// Functions in this module are used by different test binaries; each binary
// only uses a subset, so suppress dead_code warnings.
#![allow(dead_code)]

use std::sync::Arc;

use s3_gallery_core::db::migrate::run_full_migration;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::entity::{file, file_tag, metadata, tag, thumbnail};
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::mock::MockS3Client;
use s3_gallery_core::types::{
    BucketName, Etag, FileSize, FileType, HostId, MetadataNamespace, MetadataState, ObjectKey,
    TagType, ThumbnailFormat,
};
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tempfile::TempDir;

/// Create a temporary SQLite database, run migrations, and return the connection
/// along with the temp directory handle (kept alive for the lifetime of the
/// returned `TempDir`).
pub async fn setup_test_db() -> Result<(DatabaseConnection, TempDir)> {
    let dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let db_path = dir.path().join("test.db");
    let db = create_pool(&db_path).await?;
    run_full_migration(&db).await?;
    Ok((db, dir))
}

/// Seed the database with a known set of test files.
///
/// Returns the number of files inserted.
pub async fn seed_test_files(db: &DatabaseConnection) -> Result<usize> {
    let files = vec![
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("photos/vacation/img001.jpg")?),
            etag: Set(Etag::new("etag-001")?),
            size: Set(FileSize::new(102400)),
            last_modified: Set("2026-06-01T12:00:00Z".to_string()),
            content_type: Set(Some("image/jpeg".to_string())),
            file_type: Set(FileType::Jpeg),
            metadata_state: Set(MetadataState::Extracted),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("photos/vacation/img002.jpg")?),
            etag: Set(Etag::new("etag-002")?),
            size: Set(FileSize::new(204800)),
            last_modified: Set("2026-06-02T12:00:00Z".to_string()),
            content_type: Set(Some("image/jpeg".to_string())),
            file_type: Set(FileType::Jpeg),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("photos/party/clip001.mp4")?),
            etag: Set(Etag::new("etag-003")?),
            size: Set(FileSize::new(5_242_880)),
            last_modified: Set("2026-06-03T12:00:00Z".to_string()),
            content_type: Set(Some("video/mp4".to_string())),
            file_type: Set(FileType::Mp4),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("docs/report.pdf")?),
            etag: Set(Etag::new("etag-004")?),
            size: Set(FileSize::new(307_200)),
            last_modified: Set("2026-06-04T12:00:00Z".to_string()),
            content_type: Set(Some("application/pdf".to_string())),
            file_type: Set(FileType::Pdf),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("docs/notes.txt")?),
            etag: Set(Etag::new("etag-005")?),
            size: Set(FileSize::new(5120)),
            last_modified: Set("2026-06-05T12:00:00Z".to_string()),
            content_type: Set(Some("text/plain".to_string())),
            file_type: Set(FileType::Unknown),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
    ];

    let count = files.len();
    for file_model in files {
        file::Entity::insert(file_model).exec(db).await?;
    }
    Ok(count)
}

/// Seed the database with tags and file-tag associations.
pub async fn seed_test_tags(db: &DatabaseConnection) -> Result<()> {
    let tag_active = tag::ActiveModel {
        tag_name: Set("vacation".to_string()),
        tag_type: Set(TagType::Manual),
        ..Default::default()
    };
    tag::Entity::insert(tag_active).exec(db).await?;

    let tag = tag::Entity::find()
        .filter(tag::Column::TagName.eq("vacation"))
        .one(db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("tag 'vacation'".into()))?;

    let ft = file_tag::ActiveModel {
        file_key: Set(ObjectKey::new("photos/vacation/img001.jpg")?),
        tag_id: Set(tag.tag_id),
    };
    file_tag::Entity::insert(ft).exec(db).await?;

    let ft = file_tag::ActiveModel {
        file_key: Set(ObjectKey::new("photos/vacation/img002.jpg")?),
        tag_id: Set(tag.tag_id),
    };
    file_tag::Entity::insert(ft).exec(db).await?;

    Ok(())
}

/// Seed the database with metadata entries.
pub async fn seed_test_metadata(db: &DatabaseConnection) -> Result<()> {
    let meta = metadata::ActiveModel {
        file_key: Set(ObjectKey::new("photos/vacation/img001.jpg")?),
        namespace: Set(MetadataNamespace::Exif),
        namespace_custom: Set(None),
        key: Set("Make".to_string()),
        value: Set("Canon".to_string()),
        extracted_at: Set("2026-06-01T12:00:00Z".to_string()),
        partial: Set(false),
    };
    metadata::Entity::insert(meta).exec(db).await?;

    let meta = metadata::ActiveModel {
        file_key: Set(ObjectKey::new("photos/vacation/img001.jpg")?),
        namespace: Set(MetadataNamespace::Exif),
        namespace_custom: Set(None),
        key: Set("Model".to_string()),
        value: Set("EOS R5".to_string()),
        extracted_at: Set("2026-06-01T12:00:00Z".to_string()),
        partial: Set(false),
    };
    metadata::Entity::insert(meta).exec(db).await?;

    Ok(())
}

/// Seed the database with a thumbnail entry.
pub async fn seed_test_thumbnail(db: &DatabaseConnection) -> Result<()> {
    let thumb = thumbnail::ActiveModel {
        file_key: Set(ObjectKey::new("photos/vacation/img001.jpg")?),
        data: Set(vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]),
        format: Set(ThumbnailFormat::Jpeg),
        width: Set(Some(150)),
        height: Set(Some(150)),
        cached_at: Set("2026-06-01T12:00:00Z".to_string()),
    };
    thumbnail::Entity::insert(thumb).exec(db).await?;
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
