//! Integration tests for database CRUD operations and schema.
//!
//! Tests full CRUD for all model types, schema migration idempotency,
//! and WAL journal mode.

use s3_gallery_core::db::models::{
    FileEntry, FileTagEntry, HostConfigEntry, MetadataEntry, ScanMetadata, TagEntry, ThumbnailEntry,
};
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::error::{Result, S3GalleryError};
use sea_orm::DatabaseConnection;
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
    run_migrations(db.get_sqlite_connection_pool()).await?;
    Ok((db, dir))
}

/// Insert a minimal file entry for use as a foreign key target.
async fn insert_base_file(pool: &sqlx::SqlitePool) -> Result<()> {
    FileEntry::insert(
        pool,
        &FileEntry {
            host_id: "test-host".to_string(),
            key: "base/file.jpg".to_string(),
            etag: "base-etag".to_string(),
            size: 100,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Schema tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_schema_migration_creates_all_tables() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

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
    let pool = db.get_sqlite_connection_pool();

    // Run migrations twice.
    run_migrations(pool).await?;

    // Tables should still exist.
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .fetch_all(pool)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    assert!(tables.contains(&"files".to_string()));
    assert!(tables.contains(&"metadata".to_string()));
    Ok(())
}

#[tokio::test]
async fn test_wal_mode_is_enabled() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode;")
        .fetch_one(pool)
        .await
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
    let pool = db.get_sqlite_connection_pool();

    let fk_enabled: i32 = sqlx::query_scalar("PRAGMA foreign_keys;")
        .fetch_one(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    assert_eq!(fk_enabled, 1, "foreign keys should be enabled");
    Ok(())
}

#[tokio::test]
async fn test_schema_version_is_set() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    let version: i64 = sqlx::query_scalar("SELECT db_schema_version FROM scan_metadata LIMIT 1")
        .fetch_one(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    assert_eq!(version, 2);
    Ok(())
}

// ---------------------------------------------------------------------------
// HostConfigEntry CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_host_config_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    let entry = HostConfigEntry {
        host_id: "integration-host".to_string(),
        host_name: "Integration Test Host".to_string(),
        host_type: "s3".to_string(),
        description: "Created during integration test".to_string(),
        created_at: "2026-07-01T00:00:00Z".to_string(),
        bucket: "".to_string(),
        endpoint: "".to_string(),
        region: "".to_string(),
    };

    // Create
    HostConfigEntry::insert(&pool, &entry).await?;

    // Read
    let fetched = HostConfigEntry::get(&pool, "integration-host").await?;
    assert_eq!(fetched.host_name, "Integration Test Host");
    assert_eq!(fetched.host_type, "s3");

    // Update
    let updated = HostConfigEntry {
        description: "Updated description".to_string(),
        ..entry
    };
    HostConfigEntry::update(&pool, &updated).await?;
    let fetched = HostConfigEntry::get(&pool, "integration-host").await?;
    assert_eq!(fetched.description, "Updated description");

    // Delete
    HostConfigEntry::delete(&pool, "integration-host").await?;
    let result = HostConfigEntry::get(&pool, "integration-host").await;
    assert!(result.is_err());
    assert!(matches!(result, Err(S3GalleryError::NotFound(_))));
    Ok(())
}

#[tokio::test]
async fn test_host_config_get_not_found() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();
    let result = HostConfigEntry::get(&pool, "nonexistent").await;
    assert!(result.is_err());
    assert!(matches!(result, Err(S3GalleryError::NotFound(_))));
    Ok(())
}

// ---------------------------------------------------------------------------
// FileEntry CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_file_entry_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    let entry = FileEntry {
        host_id: "test-host".to_string(),
        key: "integration/test.jpg".to_string(),
        etag: "test-etag-123".to_string(),
        size: 5555,
        last_modified: "2026-07-15T10:30:00Z".to_string(),
        content_type: Some("image/jpeg".to_string()),
        file_type: "jpeg".to_string(),
        metadata_state: "pending".to_string(),
        effective_date: "".to_string(),
        is_deleted: false,
    };

    // Create
    FileEntry::insert(&pool, &entry).await?;

    // Read
    let fetched = FileEntry::get_by_key(&pool, "test-host", "integration/test.jpg").await?;
    assert_eq!(fetched.etag, "test-etag-123");
    assert_eq!(fetched.size, 5555);
    assert_eq!(fetched.content_type, Some("image/jpeg".to_string()));

    // Upsert
    let upserted = FileEntry {
        etag: "updated-etag".to_string(),
        size: 6666,
        ..entry
    };
    FileEntry::upsert(&pool, &upserted).await?;
    let fetched = FileEntry::get_by_key(&pool, "test-host", "integration/test.jpg").await?;
    assert_eq!(fetched.etag, "updated-etag");
    assert_eq!(fetched.size, 6666);

    // Soft delete
    FileEntry::mark_deleted(&pool, "test-host", "integration/test.jpg").await?;
    let fetched = FileEntry::get_by_key(&pool, "test-host", "integration/test.jpg").await?;
    assert!(fetched.is_deleted);

    Ok(())
}

#[tokio::test]
async fn test_file_entry_list_by_prefix() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    let files = vec![
        FileEntry {
            host_id: "test-host".to_string(),
            key: "prefix/a.txt".to_string(),
            etag: "e1".to_string(),
            size: 10,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: None,
            file_type: "txt".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
        FileEntry {
            host_id: "test-host".to_string(),
            key: "prefix/b.txt".to_string(),
            etag: "e2".to_string(),
            size: 20,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: None,
            file_type: "txt".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
        FileEntry {
            host_id: "test-host".to_string(),
            key: "other/c.txt".to_string(),
            etag: "e3".to_string(),
            size: 30,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: None,
            file_type: "txt".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
    ];
    for f in &files {
        FileEntry::insert(&pool, f).await?;
    }

    let results = FileEntry::list_by_prefix(&pool, "test-host", "prefix/").await?;
    assert_eq!(results.len(), 2);

    let results = FileEntry::list_by_prefix(&pool, "test-host", "nonexistent/").await?;
    assert!(results.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_file_entry_list_by_file_type() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    let files = vec![
        FileEntry {
            host_id: "test-host".to_string(),
            key: "img.jpg".to_string(),
            etag: "e1".to_string(),
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
            etag: "e2".to_string(),
            size: 200,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("application/pdf".to_string()),
            file_type: "pdf".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
    ];
    for f in &files {
        FileEntry::insert(&pool, f).await?;
    }

    let results = FileEntry::list_by_file_type(&pool, "test-host", "jpeg").await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key, "img.jpg");
    Ok(())
}

#[tokio::test]
async fn test_file_entry_count() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    assert_eq!(FileEntry::count(&pool, "test-host").await?, 0);

    FileEntry::insert(
        &pool,
        &FileEntry {
            host_id: "test-host".to_string(),
            key: "counted.txt".to_string(),
            etag: "e1".to_string(),
            size: 10,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: None,
            file_type: "txt".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
    )
    .await?;
    assert_eq!(FileEntry::count(&pool, "test-host").await?, 1);

    FileEntry::mark_deleted(&pool, "test-host", "counted.txt").await?;
    assert_eq!(FileEntry::count(&pool, "test-host").await?, 0);

    Ok(())
}

#[tokio::test]
async fn test_file_entry_get_not_found() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();
    let result = FileEntry::get_by_key(&pool, "test-host", "nonexistent").await;
    assert!(result.is_err());
    assert!(matches!(result, Err(S3GalleryError::NotFound(_))));
    Ok(())
}

// ---------------------------------------------------------------------------
// MetadataEntry CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_metadata_entry_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();
    insert_base_file(&pool).await?;

    let meta = MetadataEntry {
        file_key: "base/file.jpg".to_string(),
        namespace: "exif".to_string(),
        key: "ISOSpeedRatings".to_string(),
        value: "400".to_string(),
        extracted_at: "2026-07-01T00:00:00Z".to_string(),
        partial: false,
    };

    // Create
    MetadataEntry::insert(&pool, &meta).await?;

    // Read by file key
    let results = MetadataEntry::get_by_file_key(&pool, "base/file.jpg").await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key, "ISOSpeedRatings");
    assert_eq!(results[0].value, "400");

    // Read by namespace
    let results = MetadataEntry::get_by_namespace(&pool, "exif").await?;
    assert_eq!(results.len(), 1);

    // Delete
    MetadataEntry::delete_by_file_key(&pool, "base/file.jpg").await?;
    let results = MetadataEntry::get_by_file_key(&pool, "base/file.jpg").await?;
    assert!(results.is_empty());

    Ok(())
}

// ---------------------------------------------------------------------------
// ThumbnailEntry CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_thumbnail_entry_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();
    insert_base_file(&pool).await?;

    let thumb = ThumbnailEntry {
        file_key: "base/file.jpg".to_string(),
        data: vec![0xFF, 0xD8, 0xFF, 0xE0],
        format: "jpeg".to_string(),
        width: Some(320),
        height: Some(240),
        cached_at: "2026-07-01T00:00:00Z".to_string(),
    };

    // Create
    ThumbnailEntry::insert(&pool, &thumb).await?;

    // Read
    let fetched = ThumbnailEntry::get(&pool, "base/file.jpg").await?;
    assert_eq!(fetched.format, "jpeg");
    assert_eq!(fetched.width, Some(320));
    assert_eq!(fetched.height, Some(240));

    // Update
    ThumbnailEntry::update(
        &pool,
        &ThumbnailEntry {
            width: Some(640),
            height: Some(480),
            ..thumb
        },
    )
    .await?;
    let fetched = ThumbnailEntry::get(&pool, "base/file.jpg").await?;
    assert_eq!(fetched.width, Some(640));
    assert_eq!(fetched.height, Some(480));

    // Delete
    ThumbnailEntry::delete(&pool, "base/file.jpg").await?;
    let result = ThumbnailEntry::get(&pool, "base/file.jpg").await;
    assert!(result.is_err());
    assert!(matches!(result, Err(S3GalleryError::NotFound(_))));

    Ok(())
}

// ---------------------------------------------------------------------------
// TagEntry CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_tag_entry_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    let tag = TagEntry {
        tag_id: 0,
        tag_name: "integration-test-tag".to_string(),
        tag_type: "manual".to_string(),
    };

    // Create
    TagEntry::insert(&pool, &tag).await?;

    // Read by name
    let fetched = TagEntry::get_by_name(&pool, "integration-test-tag").await?;
    assert_eq!(fetched.tag_name, "integration-test-tag");
    assert_eq!(fetched.tag_type, "manual");
    assert!(fetched.tag_id > 0);

    // List all
    let tags = TagEntry::list_all(&pool).await?;
    assert_eq!(tags.len(), 1);

    // Get by name not found
    let result = TagEntry::get_by_name(&pool, "nonexistent").await;
    assert!(result.is_err());
    assert!(matches!(result, Err(S3GalleryError::NotFound(_))));

    Ok(())
}

// ---------------------------------------------------------------------------
// FileTagEntry CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_file_tag_full_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();
    insert_base_file(&pool).await?;

    // Create tag
    TagEntry::insert(
        &pool,
        &TagEntry {
            tag_id: 0,
            tag_name: "test-tag".to_string(),
            tag_type: "auto".to_string(),
        },
    )
    .await?;
    let tag = TagEntry::get_by_name(&pool, "test-tag").await?;

    // Create association
    let ft = FileTagEntry {
        file_key: "base/file.jpg".to_string(),
        tag_id: tag.tag_id,
    };
    FileTagEntry::insert(&pool, &ft).await?;

    // Read by file key
    let results = FileTagEntry::get_by_file_key(&pool, "base/file.jpg").await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tag_id, tag.tag_id);

    // Delete
    FileTagEntry::delete(&pool, "base/file.jpg", tag.tag_id).await?;
    let results = FileTagEntry::get_by_file_key(&pool, "base/file.jpg").await?;
    assert!(results.is_empty());

    Ok(())
}

// ---------------------------------------------------------------------------
// ScanMetadata CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scan_metadata_crud() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    // Initial state after migration
    let fetched = ScanMetadata::get(&pool, "default").await?;
    assert_eq!(fetched.db_schema_version, 2);
    assert!(fetched.last_scanned_key.is_none());

    // Update
    let updated = ScanMetadata {
        host_id: "default".to_string(),
        last_scanned_key: Some("integration/last-file.txt".to_string()),
        last_scanned_at: Some("2026-07-20T00:00:00Z".to_string()),
        total_files: Some(42),
        total_size: Some(1048576),
        db_schema_version: 2,
    };
    ScanMetadata::update(&pool, &updated).await?;

    let fetched = ScanMetadata::get(&pool, "default").await?;
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
    let pool = db.get_sqlite_connection_pool();

    // Metadata no longer has a foreign key constraint on files, so inserting
    // metadata without a corresponding file entry should succeed.
    let meta = MetadataEntry {
        file_key: "orphan-file".to_string(),
        namespace: "exif".to_string(),
        key: "Make".to_string(),
        value: "Canon".to_string(),
        extracted_at: "2026-01-01T00:00:00Z".to_string(),
        partial: false,
    };
    let result = MetadataEntry::insert(&pool, &meta).await;
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
    let pool = db.get_sqlite_connection_pool();
    insert_base_file(&pool).await?;

    // Insert metadata referencing the file.
    let meta = MetadataEntry {
        file_key: "base/file.jpg".to_string(),
        namespace: "general".to_string(),
        key: "note".to_string(),
        value: "test".to_string(),
        extracted_at: "2026-01-01T00:00:00Z".to_string(),
        partial: false,
    };
    MetadataEntry::insert(&pool, &meta).await?;

    // Soft-delete the file.
    FileEntry::mark_deleted(&pool, "test-host", "base/file.jpg").await?;

    // Metadata should still exist (soft delete doesn't cascade).
    let results = MetadataEntry::get_by_file_key(&pool, "base/file.jpg").await?;
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

    // Clean up.
    db.close()
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
    Ok(())
}

#[tokio::test]
async fn test_pool_accepts_multiple_connections() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    // Run a query on the pool to verify it works.
    let result: i64 = sqlx::query_scalar("SELECT 1 + 1")
        .fetch_one(pool)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    assert_eq!(result, 2);
    Ok(())
}

#[tokio::test]
async fn test_all_indexes_created() -> Result<()> {
    let (db, _dir) = setup_db().await?;
    let pool = db.get_sqlite_connection_pool();

    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='index' AND name IS NOT NULL ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let expected = [
        "idx_files_file_type",
        "idx_files_last_modified",
        "idx_metadata_file_key",
        "idx_metadata_key_value",
        "idx_metadata_namespace",
        "idx_tags_tag_type",
        "idx_thumbnails_cached_at",
    ];

    for name in &expected {
        assert!(
            indexes.contains(&name.to_string()),
            "index {name} should exist"
        );
    }

    Ok(())
}
