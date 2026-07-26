//! Integration tests for the full scan flow.
//!
//! Tests Scanner with a MockS3Client, verifying that S3 objects are correctly
//! discovered, diffed against the DB, and persisted.

use std::sync::Arc;

use s3_gallery_core::entity::file;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::mock::MockS3Client;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::scan::scanner::{run_scan, ScanConfig};
use s3_gallery_core::types::{
    BucketName, Etag, FileSize, FileType, HostId, MetadataState, ObjectKey, Prefix,
};
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use sea_orm::DatabaseConnection;

mod common;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a ScanConfig for testing.
fn make_scan_config(
    s3: Arc<dyn S3Client>,
    db: DatabaseConnection,
    bucket: BucketName,
    prefix: &str,
) -> ScanConfig {
    // The test objects are placed under "test/" so we use "test/" as the listing prefix.
    let listing_prefix = if prefix.is_empty() { "test/" } else { prefix };
    ScanConfig {
        host_id: "test-host".to_string(),
        s3: S3Service::new(s3),
        db: db.get_sqlite_connection_pool().clone(),
        sea_db: db,
        bucket,
        prefix: Prefix::new(listing_prefix).expect("valid prefix"),
        concurrency: 10,
        extract_metadata: false,
        generate_thumbnails: false,
        client_id: "test-client".to_string(),
    }
}

/// Generate a valid host.config.json for the test scope prefix.
/// The config key stored at `test/.s3-gallery/host.config.json` tells
/// DiscoverLayer to use "test-host" as the single host.
fn host_config_bytes() -> &'static [u8] {
    br#"{"host_id":"test-host","host_name":"test-host","host_type":"test","description":"","bucket":"test-bucket","prefix":"","created_at":"2026-01-01T00:00:00Z","version":1,"db_path":"s3-gallery.db","lock_path":"s3-gallery.lock","config_path":".s3-gallery/host.config.json","s3_gallery_dir":".s3-gallery"}"#
}

/// Create a MockS3Client with test fixtures plus a host.config.json
/// so DiscoverLayer finds a single host ("test-host") instead of
/// discovering hosts from subdirectories.
fn mock_s3_with_fixtures(objects: Vec<(&str, &[u8])>) -> Result<MockS3Client> {
    let mut all = Vec::with_capacity(objects.len() + 1);
    all.push(("test/.s3-gallery/host.config.json", host_config_bytes()));
    for (k, v) in objects {
        all.push((k, v));
    }
    MockS3Client::with_fixtures(all)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scan_empty_bucket() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;
    let bucket = common::test_bucket()?;

    let config = make_scan_config(s3, db, bucket, "");
    let result = run_scan(config, String::new()).await?;

    assert_eq!(result.total_files, 0);
    assert_eq!(result.new_files, 0);
    assert_eq!(result.changed_files, 0);
    assert_eq!(result.deleted_files, 0);
    Ok(())
}

#[tokio::test]
async fn test_scan_discovers_new_objects() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    let s3 = mock_s3_with_fixtures(vec![
        ("test/photos/img001.jpg", b"jpeg data"),
        ("test/photos/img002.jpg", b"jpeg data"),
        ("test/docs/readme.txt", b"text data"),
    ])?;
    let bucket = common::test_bucket()?;

    let config = make_scan_config(Arc::new(s3), db.clone(), bucket, "");
    let result = run_scan(config, "".to_owned()).await?;

    assert_eq!(result.total_files, 3, "should discover 3 objects");
    assert_eq!(result.new_files, 3, "all 3 should be new");

    // Verify DB entries were created.
    let count = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .count(&db)
        .await?;
    assert_eq!(count, 3);

    // Verify specific entries.
    let entry = file::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(file::Column::HostId.eq(HostId::new("test-host")?))
                .add(file::Column::Key.eq(ObjectKey::new("test/photos/img001.jpg")?)),
        )
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?;
    assert_eq!(entry.file_type, FileType::Jpeg);
    assert!(!entry.is_deleted);

    let entry = file::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(file::Column::HostId.eq(HostId::new("test-host")?))
                .add(file::Column::Key.eq(ObjectKey::new("test/docs/readme.txt")?)),
        )
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?;
    // "txt" is not a recognized file type, so it's classified as "unknown".
    assert_eq!(entry.file_type, FileType::Unknown);
    Ok(())
}

#[tokio::test]
async fn test_scan_detects_modified_objects() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    let bucket = common::test_bucket()?;

    // Insert a file entry with an old etag.
    let am = file::ActiveModel {
        host_id: Set(HostId::new("test-host")?),
        key: Set(ObjectKey::new("test/photos/img001.jpg")?),
        etag: Set(Etag::new("old-etag")?),
        size: Set(FileSize::new(100)),
        last_modified: Set("2026-01-01T00:00:00Z".to_string()),
        content_type: Set(Some("image/jpeg".to_string())),
        file_type: Set(FileType::Jpeg),
        metadata_state: Set(MetadataState::Pending),
        is_deleted: Set(false),
        effective_date: Set("".to_string()),
    };
    file::Entity::insert(am).exec(&db).await?;

    // The mock S3 generates a random etag on insert, so the object will have
    // a different etag than what's in the DB, triggering a "changed" detection.
    let s3 = mock_s3_with_fixtures(vec![("test/photos/img001.jpg", b"updated jpeg data")])?;
    let config = make_scan_config(Arc::new(s3), db.clone(), bucket, "");
    let result = run_scan(config, String::new()).await?;

    assert_eq!(result.total_files, 1);
    assert_eq!(result.new_files, 0);
    assert_eq!(
        result.changed_files, 1,
        "etag differs, should be detected as changed"
    );
    assert_eq!(result.deleted_files, 0);

    // Verify the etag was updated in the DB.
    let updated = file::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(file::Column::HostId.eq(HostId::new("test-host")?))
                .add(file::Column::Key.eq(ObjectKey::new("test/photos/img001.jpg")?)),
        )
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?;
    assert_ne!(
        updated.etag,
        Etag::new("old-etag")?,
        "etag should have been updated"
    );
    assert_eq!(
        updated.size,
        FileSize::new(17),
        "size should match 'updated jpeg data' len"
    );
    Ok(())
}

#[tokio::test]
async fn test_scan_detects_deleted_objects() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    let bucket = common::test_bucket()?;

    // Insert a file that exists in the DB but not in S3.
    let am = file::ActiveModel {
        host_id: Set(HostId::new("test-host")?),
        key: Set(ObjectKey::new("test/photos/ghost.txt")?),
        etag: Set(Etag::new("ghost-etag")?),
        size: Set(FileSize::new(50)),
        last_modified: Set("2026-01-01T00:00:00Z".to_string()),
        content_type: Set(None),
        file_type: Set(FileType::Unknown),
        metadata_state: Set(MetadataState::Pending),
        is_deleted: Set(false),
        effective_date: Set("".to_string()),
    };
    file::Entity::insert(am).exec(&db).await?;

    let s3 = mock_s3_with_fixtures(vec![])?; // empty — no objects
    let config = make_scan_config(Arc::new(s3), db.clone(), bucket, "");
    let result = run_scan(config, String::new()).await?;

    assert_eq!(result.total_files, 0);
    assert_eq!(result.new_files, 0);
    assert_eq!(result.changed_files, 0);
    assert_eq!(
        result.deleted_files, 1,
        "ghost.txt should be marked deleted"
    );

    // Verify the file is soft-deleted.
    let entry = file::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(file::Column::HostId.eq(HostId::new("test-host")?))
                .add(file::Column::Key.eq(ObjectKey::new("test/photos/ghost.txt")?)),
        )
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?;
    assert!(entry.is_deleted);
    Ok(())
}

#[tokio::test]
async fn test_scan_mixed_new_changed_deleted() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    let bucket = common::test_bucket()?;

    // Pre-populate DB with one unchanged, one that will be "changed" (different
    // etag), and one that will be "deleted" (not in S3).
    let db_files = vec![
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("test/unchanged.txt")?),
            etag: Set(Etag::new("u-etag")?),
            size: Set(FileSize::new(10)),
            last_modified: Set("2026-01-01T00:00:00Z".to_string()),
            content_type: Set(None),
            file_type: Set(FileType::Unknown),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("test/changed.txt")?),
            etag: Set(Etag::new("old-etag")?),
            size: Set(FileSize::new(20)),
            last_modified: Set("2026-01-01T00:00:00Z".to_string()),
            content_type: Set(None),
            file_type: Set(FileType::Unknown),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("test/deleted.txt")?),
            etag: Set(Etag::new("d-etag")?),
            size: Set(FileSize::new(30)),
            last_modified: Set("2026-01-01T00:00:00Z".to_string()),
            content_type: Set(None),
            file_type: Set(FileType::Unknown),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
    ];
    for f in &db_files {
        // Need to clone the ActiveModel to avoid move issues in the loop.
        // We create a fresh ActiveModel for each insert.
        file::Entity::insert(f.clone()).exec(&db).await?;
    }

    // S3 has: unchanged.txt, changed.txt, and new.txt (not in DB).
    // Note: MockS3Client generates random etags, so the etag for unchanged.txt
    // will differ from "u-etag", making it "changed" too.
    let s3 = mock_s3_with_fixtures(vec![
        ("test/unchanged.txt", b"data"),
        ("test/changed.txt", b"data"),
        ("test/new.txt", b"new data"),
    ])?;
    let config = make_scan_config(Arc::new(s3), db.clone(), bucket, "");
    let result = run_scan(config, String::new()).await?;

    // At minimum we should see:
    assert_eq!(result.total_files, 3, "3 objects in S3");
    assert_eq!(result.new_files, 1, "new.txt is new");
    assert_eq!(result.deleted_files, 1, "deleted.txt was removed");

    // Verify new.txt was inserted.
    let new_entry = file::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(file::Column::HostId.eq(HostId::new("test-host")?))
                .add(file::Column::Key.eq(ObjectKey::new("test/new.txt")?)),
        )
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?;
    assert!(!new_entry.is_deleted);

    // Verify deleted.txt was soft-deleted.
    let deleted_entry = file::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(file::Column::HostId.eq(HostId::new("test-host")?))
                .add(file::Column::Key.eq(ObjectKey::new("test/deleted.txt")?)),
        )
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?;
    assert!(deleted_entry.is_deleted);

    Ok(())
}

#[tokio::test]
async fn test_scan_updates_scan_metadata() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    let s3 = mock_s3_with_fixtures(vec![("test/a.jpg", b"data"), ("test/b.jpg", b"data")])?;

    let config = make_scan_config(Arc::new(s3), db, common::test_bucket()?, "");
    let result = run_scan(config, String::new()).await?;

    assert_eq!(result.total_files, 2);
    assert_eq!(result.total_size, 8); // 2 * b"data".len()

    Ok(())
}

#[tokio::test]
async fn test_scan_skips_s3_gallery_directory() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    let s3 = mock_s3_with_fixtures(vec![("test/photos/img.jpg", b"data")])?;

    let config = make_scan_config(Arc::new(s3), db.clone(), common::test_bucket()?, "");
    let result = run_scan(config, String::new()).await?;

    // Only the non-.s3-gallery file should be counted.
    assert_eq!(
        result.total_files, 1,
        "s3-gallery directory should be filtered out"
    );
    assert_eq!(result.new_files, 1);

    // Verify the photo file was recorded.
    let entry = file::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(file::Column::HostId.eq(HostId::new("test-host")?))
                .add(file::Column::Key.eq(ObjectKey::new("test/photos/img.jpg")?)),
        )
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?;
    assert!(!entry.is_deleted);
    Ok(())
}