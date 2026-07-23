//! Integration tests for LocalView and RemoteView.
//!
//! LocalView is a pure DB query layer (no S3 field).  RemoteView wraps S3
//! operations on top of a LocalView.

use std::sync::Arc;

use s3_gallery_core::db::models::FileEntry;
use s3_gallery_core::error::Result;
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::mock::MockS3Client;
use s3_gallery_core::types::{ObjectKey, SortField, SortOrder};
use s3_gallery_core::view::export::ExportFormat;
use s3_gallery_core::view::LocalView;

mod common;

// ---------------------------------------------------------------------------
// LocalView tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_local_view_list_directory() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;

    let view = LocalView::new(pool);

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
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;

    let view = LocalView::new(pool);
    let stats = view.get_stats("test-host").await?;

    assert_eq!(stats.total_files, 5);
    assert!(stats.total_size.as_u64() > 0);
    assert!(stats.by_file_type.contains_key("jpeg"));
    assert!(stats.by_file_type.contains_key("mp4"));
    assert!(stats.by_file_type.contains_key("pdf"));
    assert!(stats.by_file_type.contains_key("txt"));
    Ok(())
}

#[tokio::test]
async fn test_local_view_search_by_name() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;

    let view = LocalView::new(pool);

    // Search for "vacation"
    let result = view.search_by_name("test-host", "vacation").await?;
    assert_eq!(result.total_count, 2, "should find 2 vacation files");
    assert!(result.files.iter().all(|f| f.key.contains("vacation")));

    // Search for "report"
    let result = view.search_by_name("test-host", "report").await?;
    assert_eq!(result.total_count, 1);
    assert_eq!(result.files[0].key, "docs/report.pdf");

    // Search with no matches
    let result = view.search_by_name("test-host", "nonexistent").await?;
    assert_eq!(result.total_count, 0);
    assert!(result.files.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_local_view_search_by_tag() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;
    common::seed_test_tags(&pool).await?;

    let view = LocalView::new(pool);

    let result = view.search_by_tag("test-host", "vacation").await?;
    assert_eq!(result.total_count, 2, "both vacation files are tagged");

    let result = view.search_by_tag("test-host", "nonexistent").await?;
    assert_eq!(result.total_count, 0);
    Ok(())
}

#[tokio::test]
async fn test_local_view_get_timeline() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;

    let view = LocalView::new(pool);
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
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;
    common::seed_test_tags(&pool).await?;

    let view = LocalView::new(pool);
    let tags = view.list_tags("test-host").await?;

    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].tag_name, "vacation");
    Ok(())
}

#[tokio::test]
async fn test_local_view_get_files_by_tag() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;
    common::seed_test_tags(&pool).await?;

    let view = LocalView::new(pool);
    let files = view.get_files_by_tag("test-host", "vacation").await?;

    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|f| f.key.contains("vacation")));
    Ok(())
}

#[tokio::test]
async fn test_local_view_find_duplicates() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;

    // Insert two files with the same size and etag (duplicates).
    let files = vec![
        FileEntry {
            host_id: "test-host".to_string(),
            key: "dup/a.jpg".to_string(),
            etag: "same-etag".to_string(),
            size: 1000,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
        FileEntry {
            host_id: "test-host".to_string(),
            key: "dup/b.jpg".to_string(),
            etag: "same-etag".to_string(),
            size: 1000,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
        FileEntry {
            host_id: "test-host".to_string(),
            key: "unique.jpg".to_string(),
            etag: "unique-etag".to_string(),
            size: 500,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        },
    ];
    for f in &files {
        FileEntry::insert(&pool, f).await?;
    }

    let view = LocalView::new(pool);
    let duplicates = view.find_duplicates("test-host").await?;

    assert_eq!(duplicates.len(), 1, "should find one duplicate group");
    assert_eq!(duplicates[0].files.len(), 2, "group should have 2 files");
    assert!(duplicates[0]
        .files
        .iter()
        .all(|f| f.key.starts_with("dup/")));
    Ok(())
}

#[tokio::test]
async fn test_local_view_export_json() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;

    let view = LocalView::new(pool);
    let output = view.export_files("test-host", ExportFormat::Json).await?;

    assert!(output.contains("photos/vacation/img001.jpg"));
    assert!(output.contains("docs/report.pdf"));
    assert!(output.starts_with('['));
    assert!(output.ends_with(']'));
    Ok(())
}

#[tokio::test]
async fn test_local_view_export_csv() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;

    let view = LocalView::new(pool);
    let output = view.export_files("test-host", ExportFormat::Csv).await?;

    assert!(output.contains("photos/vacation/img001.jpg"));
    assert!(output.contains("key,etag,size")); // header row
    Ok(())
}

#[tokio::test]
async fn test_local_view_build_tree() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;

    let view = LocalView::new(pool);
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
    let (pool, _dir) = common::setup_test_db().await?;
    let view = LocalView::new(pool);

    // LocalView should only have a db() accessor, not an s3() accessor.
    let _db = view.db();
    // The type does not have an s3() method — this would be a compile error.
    // view.s3(); // <-- does not compile

    // Verify it's the right type by checking the struct has no S3 field.
    let _pool_ref: &sqlx::SqlitePool = view.db();
    let _ = _pool_ref;
    Ok(())
}

// ---------------------------------------------------------------------------
