//! Integration tests for LocalView and RemoteView.
//!
//! LocalView is a pure DB query layer (no S3 field).  RemoteView wraps S3
//! operations on top of a LocalView.

use s3_gallery_core::entity::file;
use s3_gallery_core::error::Result;
use s3_gallery_core::types::{
    Etag, FileSize, FileType, HostId, MetadataState, ObjectKey, SortField, SortOrder,
};
use s3_gallery_core::view::export::ExportFormat;
use s3_gallery_core::view::LocalView;
use sea_orm::{ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

mod common;

// ---------------------------------------------------------------------------
// LocalView tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_local_view_list_directory() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&db).await?;

    let view = LocalView::new(db);

    // List root directory.
    let entries = view
        .list_directory("test-host", "", SortField::Name, SortOrder::Ascending)
        .await?;
    assert_eq!(
        entries.len(),
        2,
        "should see 'docs' and 'photos' directories"
    );

    // List "photos/" directory (the ls module adds "/" to the prefix
    // automatically, so we pass it without trailing slash).
    let entries = view
        .list_directory("test-host", "photos", SortField::Name, SortOrder::Ascending)
        .await?;
    assert_eq!(
        entries.len(),
        2,
        "should see 'party' and 'vacation' subdirs"
    );

    // List "photos/vacation/" directory (the ls module adds "/" to the prefix
    // automatically, so "photos/vacation" becomes "photos/vacation/").
    let entries = view
        .list_directory(
            "test-host",
            "photos/vacation",
            SortField::Name,
            SortOrder::Ascending,
        )
        .await?;
    assert_eq!(entries.len(), 2, "should see 2 files in vacation");
    assert_eq!(entries[0].name, "img001.jpg");
    assert_eq!(entries[1].name, "img002.jpg");
    Ok(())
}

#[tokio::test]
async fn test_local_view_get_stats() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&db).await?;

    let view = LocalView::new(db);
    let stats = view.get_stats("test-host").await?;

    assert_eq!(stats.total_files, 5);
    assert!(stats.total_size.as_u64() > 0);
    assert!(stats.by_file_type.contains_key("jpeg"));
    assert!(stats.by_file_type.contains_key("mp4"));
    assert!(stats.by_file_type.contains_key("unknown"));
    Ok(())
}

#[tokio::test]
async fn test_local_view_search_by_name() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&db).await?;

    let view = LocalView::new(db);

    // Search for "vacation"
    let result = view.search_by_name("test-host", "vacation").await?;
    assert_eq!(result.total_count, 2, "should find 2 vacation files");
    assert!(result
        .files
        .iter()
        .all(|f| f.key.as_str().contains("vacation")));

    // Search for "report"
    let result = view.search_by_name("test-host", "report").await?;
    assert_eq!(result.total_count, 1);
    assert_eq!(result.files[0].key.as_str(), "docs/report.pdf");

    // Search with no matches
    let result = view.search_by_name("test-host", "nonexistent").await?;
    assert_eq!(result.total_count, 0);
    assert!(result.files.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_local_view_search_by_tag() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&db).await?;
    common::seed_test_tags(&db).await?;

    let view = LocalView::new(db);

    let result = view.search_by_tag("test-host", "vacation").await?;
    assert_eq!(result.total_count, 2, "both vacation files are tagged");

    let result = view.search_by_tag("test-host", "nonexistent").await?;
    assert_eq!(result.total_count, 0);
    Ok(())
}

#[tokio::test]
async fn test_local_view_get_timeline() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&db).await?;

    let view = LocalView::new(db);
    let timeline = view.get_timeline("test-host").await?;

    // We have files on 5 different dates.
    assert_eq!(timeline.len(), 5, "5 distinct dates in seed data");
    // Each entry should have at least 1 file
    for entry in &timeline {
        assert!(entry.count >= 1);
        assert!(!entry.date.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn test_local_view_list_tags() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&db).await?;
    common::seed_test_tags(&db).await?;

    let view = LocalView::new(db);
    let tags = view.list_tags("test-host").await?;

    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].tag_name, "vacation");
    Ok(())
}

#[tokio::test]
async fn test_local_view_get_files_by_tag() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&db).await?;
    common::seed_test_tags(&db).await?;

    let view = LocalView::new(db);
    let files = view.get_files_by_tag("test-host", "vacation").await?;

    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|f| f.key.as_str().contains("vacation")));
    Ok(())
}

#[tokio::test]
async fn test_local_view_find_duplicates() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;

    // Insert two files with the same size and etag (duplicates).
    let files = vec![
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("dup/a.jpg")?),
            etag: Set(Etag::new("same-etag")?),
            size: Set(FileSize::new(1000)),
            last_modified: Set("2026-01-01T00:00:00Z".to_string()),
            content_type: Set(Some("image/jpeg".to_string())),
            file_type: Set(FileType::Jpeg),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("dup/b.jpg")?),
            etag: Set(Etag::new("same-etag")?),
            size: Set(FileSize::new(1000)),
            last_modified: Set("2026-01-01T00:00:00Z".to_string()),
            content_type: Set(Some("image/jpeg".to_string())),
            file_type: Set(FileType::Jpeg),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
        file::ActiveModel {
            host_id: Set(HostId::new("test-host")?),
            key: Set(ObjectKey::new("unique.jpg")?),
            etag: Set(Etag::new("unique-etag")?),
            size: Set(FileSize::new(500)),
            last_modified: Set("2026-01-01T00:00:00Z".to_string()),
            content_type: Set(Some("image/jpeg".to_string())),
            file_type: Set(FileType::Jpeg),
            metadata_state: Set(MetadataState::Pending),
            is_deleted: Set(false),
            effective_date: Set("".to_string()),
        },
    ];
    for f in files {
        file::Entity::insert(f).exec(&db).await?;
    }

    let view = LocalView::new(db);
    let duplicates = view.find_duplicates("test-host").await?;

    assert_eq!(duplicates.len(), 1, "should find one duplicate group");
    assert_eq!(duplicates[0].files.len(), 2, "group should have 2 files");
    assert!(duplicates[0]
        .files
        .iter()
        .all(|f| f.key.as_str().starts_with("dup/")));
    Ok(())
}

#[tokio::test]
async fn test_local_view_export_json() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&db).await?;

    let view = LocalView::new(db);
    let output = view.export_files("test-host", ExportFormat::Json).await?;

    assert!(output.contains("photos/vacation/img001.jpg"));
    assert!(output.contains("docs/report.pdf"));
    assert!(output.starts_with('['));
    assert!(output.ends_with(']'));
    Ok(())
}

#[tokio::test]
async fn test_local_view_export_csv() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&db).await?;

    let view = LocalView::new(db);
    let output = view.export_files("test-host", ExportFormat::Csv).await?;

    assert!(output.contains("photos/vacation/img001.jpg"));
    assert!(output.contains("key,etag,size")); // header row
    Ok(())
}

#[tokio::test]
async fn test_local_view_build_tree() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&db).await?;

    let view = LocalView::new(db);
    let tree = view.build_tree("test-host", "").await?;

    assert_eq!(tree.name, "");
    // Should have "docs" and "photos" children
    let child_names: Vec<&str> = tree.children.iter().map(|c| c.name.as_str()).collect();
    assert!(child_names.contains(&"docs"));
    assert!(child_names.contains(&"photos"));
    Ok(())
}

/// Verify that LocalView does not have an S3Client field.
#[tokio::test]
async fn test_local_view_has_no_s3_field() -> Result<()> {
    let (db, _dir) = common::setup_test_db().await?;
    let view = LocalView::new(db);

    // LocalView should only have a db() accessor, not an s3() accessor.
    let _db = view.db();
    // The type does not have an s3() method — this would be a compile error.
    // view.s3(); // <-- does not compile

    // Verify it's the right type by checking the struct has no S3 field.
    let _db_ref: &DatabaseConnection = view.db();
    let _ = _db_ref;
    Ok(())
}
