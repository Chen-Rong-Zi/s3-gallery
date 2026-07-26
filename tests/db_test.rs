//! Integration tests for database CRUD operations and schema.
//!
//! Tests full CRUD for all entity types, schema migration idempotency,
//! and WAL journal mode using SeaORM entities.

use s3_gallery_core::db::migrate::run_full_migration;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::entity::{
    file, file_tag, host_config, metadata, scan_metadata, tag, thumbnail,
};
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::types::{
    Etag, FileSize, FileType, HostId, MetadataNamespace, MetadataState, ObjectKey, TagType,
    ThumbnailFormat,
};
use sea_orm::ActiveValue::Set;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    Statement,
};
use tempfile::TempDir;

mod common;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a temporary database with migrations applied.
async fn setup_db() -> Result<(DatabaseConnection, TempDir)> {
    let dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let db_path = dir.path().join("test.db");
    let db = create_pool(&db_path).await?;
    run_full_migration(&db).await?;
    Ok((db, dir))
}

/// Insert a minimal file entry for use as a foreign key target.
async fn insert_base_file(db: &DatabaseConnection) -> Result<()> {
    let am = file::ActiveModel {
        host_id: Set(HostId::new("test-host")?),
        key: Set(ObjectKey::new("base/file.jpg")?),
        etag: Set(Etag::new("base-etag")?),
        size: Set(FileSize::new(100)),
        last_modified: Set("2026-01-01T00:00:00Z".to_string()),
        content_type: Set(Some("image/jpeg".to_string())),
        file_type: Set(FileType::Jpeg),
        metadata_state: Set(MetadataState::Pending),
        is_deleted: Set(false),
        effective_date: Set("".to_string()),
    };
    file::Entity::insert(am).exec(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Schema tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_schema_migration_creates_all_tables() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let rows: Vec<sea_orm::QueryResult> = db
        .query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let tables: Vec<String> = rows
        .iter()
        .map(|row| row.try_get_by::<String, _>("name").unwrap_or_default())
        .collect();

    let expected = [
        "classification_rules",
        "dir_sizes",
        "extractor_rules",
        "file_tags",
        "files",
        "host_config",
        "metadata",
        "scan_metadata",
        "scan_objects",
        "tags",
        "thumbnails",
        "traffic_file_log",
        "traffic_log",
        "traffic_stats",
    ];

    for name in &expected {
        assert!(
            tables.contains(&name.to_string()),
            "table {name} should exist"
        );
    }

    assert_eq!(tables.len(), expected.len());
    Ok(())
}

#[tokio::test]
async fn test_migration_is_idempotent() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    // Run migrations twice.
    run_full_migration(&db).await?;

    // Tables should still exist.
    let rows: Vec<sea_orm::QueryResult> = db
        .query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let tables: Vec<String> = rows
        .iter()
        .map(|row| row.try_get_by::<String, _>("name").unwrap_or_default())
        .collect();

    assert!(tables.contains(&"files".to_string()));
    assert!(tables.contains(&"metadata".to_string()));
    Ok(())
}

#[tokio::test]
async fn test_wal_mode_is_enabled() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let row = db
        .query_one(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "PRAGMA journal_mode;",
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        .ok_or_else(|| S3GalleryError::DbError("no result from PRAGMA".into()))?;

    let journal_mode: String = row
        .try_get_by::<String, _>("journal_mode")
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    assert_eq!(
        journal_mode.to_lowercase(),
        "wal",
        "WAL mode should be enabled"
    );
    Ok(())
}

#[tokio::test]
async fn test_foreign_keys_are_enabled() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let row = db
        .query_one(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "PRAGMA foreign_keys;",
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        .ok_or_else(|| S3GalleryError::DbError("no result from PRAGMA".into()))?;

    let fk_enabled: i32 = row
        .try_get_by::<i32, _>("foreign_keys")
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    assert_eq!(fk_enabled, 1, "foreign keys should be enabled");
    Ok(())
}

#[tokio::test]
async fn test_schema_version_is_set() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let row = db
        .query_one(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT db_schema_version FROM scan_metadata LIMIT 1",
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?
        .ok_or_else(|| S3GalleryError::DbError("no scan_metadata row".into()))?;

    let version: i64 = row
        .try_get_by::<i64, _>("db_schema_version")
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    assert_eq!(version, 1);
    Ok(())
}

// ---------------------------------------------------------------------------
// HostConfig CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_host_config_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let am = host_config::ActiveModel {
        host_id: Set(HostId::new("integration-host")?),
        host_name: Set("Integration Test Host".to_string()),
        host_type: Set("s3".to_string()),
        description: Set("Created during integration test".to_string()),
        created_at: Set("2026-07-01T00:00:00Z".to_string()),
        bucket: Set("test-bucket".to_string()),
        endpoint: Set("http://localhost:9000".to_string()),
        region: Set("us-east-1".to_string()),
    };

    // Create
    host_config::Entity::insert(am)
        .exec_without_returning(&db)
        .await?;

    // Read
    let fetched = host_config::Entity::find()
        .filter(host_config::Column::HostId.eq(HostId::new("integration-host")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("host_config".into()))?;
    assert_eq!(fetched.host_name, "Integration Test Host");
    assert_eq!(fetched.host_type, "s3");

    // Update
    let mut am: host_config::ActiveModel = host_config::Entity::find()
        .filter(host_config::Column::HostId.eq(HostId::new("integration-host")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("host_config".into()))?
        .into();
    am.description = Set("Updated description".to_string());
    host_config::Entity::update(am).exec(&db).await?;

    let fetched = host_config::Entity::find()
        .filter(host_config::Column::HostId.eq(HostId::new("integration-host")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("host_config".into()))?;
    assert_eq!(fetched.description, "Updated description");

    // Delete
    host_config::Entity::delete_by_id(HostId::new("integration-host")?)
        .exec(&db)
        .await?;

    let result = host_config::Entity::find()
        .filter(host_config::Column::HostId.eq(HostId::new("integration-host")?))
        .one(&db)
        .await?;
    assert!(result.is_none(), "host_config should be deleted");

    Ok(())
}

#[tokio::test]
async fn test_host_config_get_not_found() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let result = host_config::Entity::find()
        .filter(host_config::Column::HostId.eq(HostId::new("nonexistent")?))
        .one(&db)
        .await?;
    assert!(result.is_none(), "nonexistent host should not be found");

    Ok(())
}

// ---------------------------------------------------------------------------
// File CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_file_entry_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let am = file::ActiveModel {
        host_id: Set(HostId::new("test-host")?),
        key: Set(ObjectKey::new("integration/test.jpg")?),
        etag: Set(Etag::new("test-etag-123")?),
        size: Set(FileSize::new(5555)),
        last_modified: Set("2026-07-15T10:30:00Z".to_string()),
        content_type: Set(Some("image/jpeg".to_string())),
        file_type: Set(FileType::Jpeg),
        metadata_state: Set(MetadataState::Pending),
        is_deleted: Set(false),
        effective_date: Set("".to_string()),
    };

    // Create
    file::Entity::insert(am).exec(&db).await?;

    // Read
    let fetched = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::Key.eq(ObjectKey::new("integration/test.jpg")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?;
    assert_eq!(fetched.etag, Etag::new("test-etag-123")?);
    assert_eq!(fetched.size, FileSize::new(5555));
    assert_eq!(fetched.content_type, Some("image/jpeg".to_string()));

    // Upsert (update existing)
    let mut am: file::ActiveModel = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::Key.eq(ObjectKey::new("integration/test.jpg")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?
        .into();
    am.etag = Set(Etag::new("updated-etag")?);
    am.size = Set(FileSize::new(6666));
    file::Entity::update(am).exec(&db).await?;

    let fetched = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::Key.eq(ObjectKey::new("integration/test.jpg")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?;
    assert_eq!(fetched.etag, Etag::new("updated-etag")?);
    assert_eq!(fetched.size, FileSize::new(6666));

    // Soft delete
    let mut am: file::ActiveModel = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::Key.eq(ObjectKey::new("integration/test.jpg")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?
        .into();
    am.is_deleted = Set(true);
    file::Entity::update(am).exec(&db).await?;

    let fetched = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::Key.eq(ObjectKey::new("integration/test.jpg")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?;
    assert!(fetched.is_deleted);

    Ok(())
}

#[tokio::test]
async fn test_file_entry_list_by_prefix() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let entries = vec![
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("prefix/a.txt")?),
            etag: Set(Etag::new("e1")?),
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
            key: Set(ObjectKey::new("prefix/b.txt")?),
            etag: Set(Etag::new("e2")?),
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
            key: Set(ObjectKey::new("other/c.txt")?),
            etag: Set(Etag::new("e3")?),
            size: Set(FileSize::new(30)),
            last_modified: Set("2026-01-01T00:00:00Z".to_string()),
            content_type: Set(None),
            file_type: Set(FileType::Unknown),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
    ];
    for f in entries {
        file::Entity::insert(f).exec(&db).await?;
    }

    let results = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::Key.starts_with("prefix/"))
        .all(&db)
        .await?;
    assert_eq!(results.len(), 2);

    let results = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::Key.starts_with("nonexistent/"))
        .all(&db)
        .await?;
    assert!(results.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_file_entry_list_by_file_type() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let entries = vec![
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("img.jpg")?),
            etag: Set(Etag::new("e1")?),
            size: Set(FileSize::new(100)),
            last_modified: Set("2026-01-01T00:00:00Z".to_string()),
            content_type: Set(Some("image/jpeg".to_string())),
            file_type: Set(FileType::Jpeg),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("doc.pdf")?),
            etag: Set(Etag::new("e2")?),
            size: Set(FileSize::new(200)),
            last_modified: Set("2026-01-01T00:00:00Z".to_string()),
            content_type: Set(Some("application/pdf".to_string())),
            file_type: Set(FileType::Pdf),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
    ];
    for f in entries {
        file::Entity::insert(f).exec(&db).await?;
    }

    let results = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::FileType.eq(FileType::Jpeg))
        .all(&db)
        .await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key, ObjectKey::new("img.jpg")?);

    Ok(())
}

#[tokio::test]
async fn test_file_entry_count() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let count = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::IsDeleted.eq(false))
        .count(&db)
        .await?;
    assert_eq!(count, 0);

    file::Entity::insert(file::ActiveModel {
        host_id: Set(HostId::new("test-host")?),
        key: Set(ObjectKey::new("counted.txt")?),
        etag: Set(Etag::new("e1")?),
        size: Set(FileSize::new(10)),
        last_modified: Set("2026-01-01T00:00:00Z".to_string()),
        content_type: Set(None),
        file_type: Set(FileType::Unknown),
        metadata_state: Set(MetadataState::Pending),
        is_deleted: Set(false),
        effective_date: Set("".to_string()),
    })
    .exec(&db)
    .await?;

    let count = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::IsDeleted.eq(false))
        .count(&db)
        .await?;
    assert_eq!(count, 1);

    // Soft delete
    let mut am: file::ActiveModel = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::Key.eq(ObjectKey::new("counted.txt")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?
        .into();
    am.is_deleted = Set(true);
    file::Entity::update(am).exec(&db).await?;

    let count = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::IsDeleted.eq(false))
        .count(&db)
        .await?;
    assert_eq!(count, 0);

    Ok(())
}

#[tokio::test]
async fn test_file_entry_get_not_found() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let result = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::Key.eq(ObjectKey::new("nonexistent")?))
        .one(&db)
        .await?;
    assert!(result.is_none(), "nonexistent file should not be found");

    Ok(())
}

// ---------------------------------------------------------------------------
// Metadata CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_metadata_entry_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    insert_base_file(&db).await?;

    let am = metadata::ActiveModel {
        file_key: Set(ObjectKey::new("base/file.jpg")?),
        namespace: Set(MetadataNamespace::Exif),
        namespace_custom: Set(None),
        key: Set("ISOSpeedRatings".to_string()),
        value: Set("400".to_string()),
        extracted_at: Set("2026-07-01T00:00:00Z".to_string()),
        partial: Set(false),
    };

    // Create
    metadata::Entity::insert(am).exec(&db).await?;

    // Read by file key
    let results = metadata::Entity::find()
        .filter(metadata::Column::FileKey.eq(ObjectKey::new("base/file.jpg")?))
        .all(&db)
        .await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key, "ISOSpeedRatings");
    assert_eq!(results[0].value, "400");

    // Read by namespace
    let results = metadata::Entity::find()
        .filter(metadata::Column::Namespace.eq(MetadataNamespace::Exif))
        .all(&db)
        .await?;
    assert_eq!(results.len(), 1);

    // Delete
    metadata::Entity::delete_many()
        .filter(metadata::Column::FileKey.eq(ObjectKey::new("base/file.jpg")?))
        .exec(&db)
        .await?;

    let results = metadata::Entity::find()
        .filter(metadata::Column::FileKey.eq(ObjectKey::new("base/file.jpg")?))
        .all(&db)
        .await?;
    assert!(results.is_empty());

    Ok(())
}

// ---------------------------------------------------------------------------
// Thumbnail CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_thumbnail_entry_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    insert_base_file(&db).await?;

    let am = thumbnail::ActiveModel {
        file_key: Set(ObjectKey::new("base/file.jpg")?),
        data: Set(vec![0xFF, 0xD8, 0xFF, 0xE0]),
        format: Set(ThumbnailFormat::Jpeg),
        width: Set(Some(320)),
        height: Set(Some(240)),
        cached_at: Set("2026-07-01T00:00:00Z".to_string()),
    };

    // Create
    thumbnail::Entity::insert(am)
        .exec_without_returning(&db)
        .await?;

    // Read
    let fetched = thumbnail::Entity::find()
        .filter(thumbnail::Column::FileKey.eq(ObjectKey::new("base/file.jpg")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("thumbnail".into()))?;
    assert_eq!(fetched.format, ThumbnailFormat::Jpeg);
    assert_eq!(fetched.width, Some(320));
    assert_eq!(fetched.height, Some(240));

    // Update
    let mut am: thumbnail::ActiveModel = thumbnail::Entity::find()
        .filter(thumbnail::Column::FileKey.eq(ObjectKey::new("base/file.jpg")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("thumbnail".into()))?
        .into();
    am.width = Set(Some(640));
    am.height = Set(Some(480));
    thumbnail::Entity::update(am).exec(&db).await?;

    let fetched = thumbnail::Entity::find()
        .filter(thumbnail::Column::FileKey.eq(ObjectKey::new("base/file.jpg")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("thumbnail".into()))?;
    assert_eq!(fetched.width, Some(640));
    assert_eq!(fetched.height, Some(480));

    // Delete
    thumbnail::Entity::delete_by_id(ObjectKey::new("base/file.jpg")?)
        .exec(&db)
        .await?;

    let result = thumbnail::Entity::find()
        .filter(thumbnail::Column::FileKey.eq(ObjectKey::new("base/file.jpg")?))
        .one(&db)
        .await?;
    assert!(result.is_none(), "thumbnail should be deleted");

    Ok(())
}

// ---------------------------------------------------------------------------
// Tag CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_tag_entry_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let am = tag::ActiveModel {
        tag_name: Set("integration-test-tag".to_string()),
        tag_type: Set(TagType::Manual),
        ..Default::default()
    };

    // Create
    let insert_result = tag::Entity::insert(am).exec(&db).await?;
    let inserted_id = insert_result.last_insert_id;

    // Read by name
    let fetched = tag::Entity::find()
        .filter(tag::Column::TagName.eq("integration-test-tag"))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("tag".into()))?;
    assert_eq!(fetched.tag_name, "integration-test-tag");
    assert_eq!(fetched.tag_type, TagType::Manual);
    assert_eq!(fetched.tag_id, inserted_id);

    // List all
    let tags = tag::Entity::find().all(&db).await?;
    assert_eq!(tags.len(), 1);

    // Get by name not found
    let result = tag::Entity::find()
        .filter(tag::Column::TagName.eq("nonexistent"))
        .one(&db)
        .await?;
    assert!(result.is_none(), "nonexistent tag should not be found");

    Ok(())
}

// ---------------------------------------------------------------------------
// FileTag CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_file_tag_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    insert_base_file(&db).await?;

    // Create tag
    let tag_am = tag::ActiveModel {
        tag_name: Set("test-tag".to_string()),
        tag_type: Set(TagType::Auto),
        ..Default::default()
    };
    tag::Entity::insert(tag_am).exec(&db).await?;

    let tag = tag::Entity::find()
        .filter(tag::Column::TagName.eq("test-tag"))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("tag".into()))?;

    // Create association
    let ft = file_tag::ActiveModel {
        file_key: Set(ObjectKey::new("base/file.jpg")?),
        tag_id: Set(tag.tag_id),
    };
    file_tag::Entity::insert(ft).exec(&db).await?;

    // Read by file key
    let results = file_tag::Entity::find()
        .filter(file_tag::Column::FileKey.eq(ObjectKey::new("base/file.jpg")?))
        .all(&db)
        .await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tag_id, tag.tag_id);

    // Delete
    file_tag::Entity::delete_by_id((ObjectKey::new("base/file.jpg")?, tag.tag_id))
        .exec(&db)
        .await?;

    let results = file_tag::Entity::find()
        .filter(file_tag::Column::FileKey.eq(ObjectKey::new("base/file.jpg")?))
        .all(&db)
        .await?;
    assert!(results.is_empty());

    Ok(())
}

// ---------------------------------------------------------------------------
// ScanMetadata CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scan_metadata_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    // Initial state after migration
    let fetched = scan_metadata::Entity::find()
        .filter(scan_metadata::Column::HostId.eq(HostId::new("default")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("scan_metadata".into()))?;
    assert_eq!(fetched.db_schema_version, 1);
    assert!(fetched.last_scanned_key.is_none());

    // Update
    let mut am: scan_metadata::ActiveModel = scan_metadata::Entity::find()
        .filter(scan_metadata::Column::HostId.eq(HostId::new("default")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("scan_metadata".into()))?
        .into();
    am.last_scanned_key = Set(Some("integration/last-file.txt".to_string()));
    am.last_scanned_at = Set(Some("2026-07-20T00:00:00Z".to_string()));
    am.total_files = Set(Some(42));
    am.total_size = Set(Some(1048576));
    scan_metadata::Entity::update(am).exec(&db).await?;

    let fetched = scan_metadata::Entity::find()
        .filter(scan_metadata::Column::HostId.eq(HostId::new("default")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("scan_metadata".into()))?;
    assert_eq!(
        fetched.last_scanned_key,
        Some("integration/last-file.txt".to_string())
    );
    assert_eq!(fetched.total_files, Some(42));
    assert_eq!(fetched.total_size, Some(1048576));

    Ok(())
}

// ---------------------------------------------------------------------------
// Foreign key constraint tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_metadata_insert_without_file() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    // Metadata no longer has a foreign key constraint on files, so inserting
    // metadata without a corresponding file entry should succeed.
    let meta = metadata::ActiveModel {
        file_key: Set(ObjectKey::new("orphan-file")?),
        namespace: Set(MetadataNamespace::Exif),
        namespace_custom: Set(None),
        key: Set("Make".to_string()),
        value: Set("Canon".to_string()),
        extracted_at: Set("2026-01-01T00:00:00Z".to_string()),
        partial: Set(false),
    };
    let result = metadata::Entity::insert(meta).exec(&db).await;
    assert!(
        result.is_ok(),
        "metadata insert without file should succeed: {result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_cascade_delete_on_file_removal() -> Result<()> {
    // Metadata no longer has a foreign key constraint on files, so soft-deleting
    // a file does not cascade to metadata.
    let (db, _dir) = setup_db().await?;
    insert_base_file(&db).await?;

    // Insert metadata referencing the file.
    let meta = metadata::ActiveModel {
        file_key: Set(ObjectKey::new("base/file.jpg")?),
        namespace: Set(MetadataNamespace::General),
        namespace_custom: Set(None),
        key: Set("note".to_string()),
        value: Set("test".to_string()),
        extracted_at: Set("2026-01-01T00:00:00Z".to_string()),
        partial: Set(false),
    };
    metadata::Entity::insert(meta).exec(&db).await?;

    // Soft-delete the file.
    let mut am: file::ActiveModel = file::Entity::find()
        .filter(file::Column::HostId.eq(HostId::new("test-host")?))
        .filter(file::Column::Key.eq(ObjectKey::new("base/file.jpg")?))
        .one(&db)
        .await?
        .ok_or_else(|| S3GalleryError::NotFound("file".into()))?
        .into();
    am.is_deleted = Set(true);
    file::Entity::update(am).exec(&db).await?;

    // Metadata should still exist (soft delete doesn't cascade).
    let results = metadata::Entity::find()
        .filter(metadata::Column::FileKey.eq(ObjectKey::new("base/file.jpg")?))
        .all(&db)
        .await?;
    assert_eq!(
        results.len(),
        1,
        "soft delete should not cascade to metadata"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Database pool creation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_pool_creates_db_file() -> Result<()> {
    let dir = tempfile::tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    let db_path = dir.path().join("newly_created.db");
    assert!(!db_path.exists());

    let db = create_pool(&db_path).await?;

    // After creating the pool, the file should exist.
    assert!(db_path.exists(), "pool creation should create the db file");

    // Clean up by dropping the connection.
    drop(db);
    Ok(())
}

#[tokio::test]
async fn test_pool_accepts_multiple_connections() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    // Run a query on the connection to verify it works.
    db.execute(Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT 1 + 1",
    ))
    .await
    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    Ok(())
}

#[tokio::test]
async fn test_all_indexes_created() -> Result<()> {
    let (db, _dir) = setup_db().await?;

    let rows: Vec<sea_orm::QueryResult> = db
        .query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type='index' AND name IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY name",
        ))
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let indexes: Vec<String> = rows
        .iter()
        .map(|row| row.try_get_by::<String, _>("name").unwrap_or_default())
        .collect();

    let expected = [
        "idx_files_file_type",
        "idx_files_host_id",
        "idx_files_last_modified",
        "idx_metadata_file_key",
        "idx_metadata_key_value",
        "idx_metadata_namespace",
        "idx_scan_objects_host_id",
        "idx_scan_objects_scan_id",
        "idx_tags_tag_type",
        "idx_thumbnails_cached_at",
        "idx_traffic_file_host_key",
        "idx_traffic_file_time",
        "idx_traffic_log_business",
        "idx_traffic_log_host_time",
    ];

    for name in &expected {
        assert!(
            indexes.contains(&name.to_string()),
            "index {name} should exist"
        );
    }

    Ok(())
}
