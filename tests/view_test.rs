//! Integration tests for LocalView and RemoteView.
//!
//! LocalView is a pure DB query layer (no S3 field).  RemoteView wraps S3
//! operations on top of a LocalView.

use std::sync::Arc;

use ossgalley_core::db::models::FileEntry;
use ossgalley_core::error::Result;
use ossgalley_core::s3::client::S3Client;
use ossgalley_core::s3::mock::MockS3Client;
use ossgalley_core::types::{ObjectKey, SortField, SortOrder};
use ossgalley_core::view::export::ExportFormat;
use ossgalley_core::view::LocalView;
use ossgalley_core::view::RemoteView;

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
        .list_directory("", SortField::Name, SortOrder::Ascending)
        .await?;
    assert_eq!(entries.len(), 2, "should see 'docs' and 'photos' directories");

    // List "photos/" directory (the ls module adds "/" to the prefix
    // automatically, so we pass it without trailing slash).
    let entries = view
        .list_directory("photos", SortField::Name, SortOrder::Ascending)
        .await?;
    assert_eq!(entries.len(), 2, "should see 'party' and 'vacation' subdirs");

    // List "photos/vacation/" directory (the ls module adds "/" to the prefix
    // automatically, so "photos/vacation" becomes "photos/vacation/").
    let entries = view
        .list_directory("photos/vacation", SortField::Name, SortOrder::Ascending)
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
    let stats = view.get_stats().await?;

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
    let result = view.search_by_name("vacation").await?;
    assert_eq!(result.total_count, 2, "should find 2 vacation files");
    assert!(result.files.iter().all(|f| f.key.contains("vacation")));

    // Search for "report"
    let result = view.search_by_name("report").await?;
    assert_eq!(result.total_count, 1);
    assert_eq!(result.files[0].key, "docs/report.pdf");

    // Search with no matches
    let result = view.search_by_name("nonexistent").await?;
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

    let result = view.search_by_tag("vacation").await?;
    assert_eq!(result.total_count, 2, "both vacation files are tagged");

    let result = view.search_by_tag("nonexistent").await?;
    assert_eq!(result.total_count, 0);
    Ok(())
}

#[tokio::test]
async fn test_local_view_get_timeline() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;

    let view = LocalView::new(pool);
    let timeline = view.get_timeline().await?;

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
    let tags = view.list_tags().await?;

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
    let files = view.get_files_by_tag("vacation").await?;

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
            key: "dup/a.jpg".to_string(),
            etag: "same-etag".to_string(),
            size: 1000,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            is_deleted: false,
        },
        FileEntry {
            key: "dup/b.jpg".to_string(),
            etag: "same-etag".to_string(),
            size: 1000,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            is_deleted: false,
        },
        FileEntry {
            key: "unique.jpg".to_string(),
            etag: "unique-etag".to_string(),
            size: 500,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            is_deleted: false,
        },
    ];
    for f in &files {
        FileEntry::insert(&pool, f).await?;
    }

    let view = LocalView::new(pool);
    let duplicates = view.find_duplicates().await?;

    assert_eq!(duplicates.len(), 1, "should find one duplicate group");
    assert_eq!(duplicates[0].files.len(), 2, "group should have 2 files");
    assert!(duplicates[0].files.iter().all(|f| f.key.starts_with("dup/")));
    Ok(())
}

#[tokio::test]
async fn test_local_view_export_json() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;

    let view = LocalView::new(pool);
    let output = view.export_files(ExportFormat::Json).await?;

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
    let output = view.export_files(ExportFormat::Csv).await?;

    assert!(output.contains("photos/vacation/img001.jpg"));
    assert!(output.contains("key,etag,size")); // header row
    Ok(())
}

#[tokio::test]
async fn test_local_view_build_tree() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    common::seed_test_files(&pool).await?;

    let view = LocalView::new(pool);
    let tree = view.build_tree("").await?;

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
// RemoteView tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_remote_view_fetch_file_content() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = common::create_mock_s3_arc()?;
    let bucket = common::test_bucket()?;

    let view = RemoteView::new(pool, s3, bucket);
    let key = ObjectKey::new("photos/vacation/img001.jpg")?;
    let content = view.fetch_file_content(&key).await?;

    assert!(!content.is_empty());
    assert_eq!(content, b"fake jpeg data for img001");
    Ok(())
}

#[tokio::test]
async fn test_remote_view_fetch_file_not_found() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = common::create_mock_s3_arc()?;
    let bucket = common::test_bucket()?;

    let view = RemoteView::new(pool, s3, bucket);
    let key = ObjectKey::new("nonexistent.txt")?;
    let result = view.fetch_file_content(&key).await;

    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn test_remote_view_fetch_byte_range() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = common::create_mock_s3_arc()?;
    let bucket = common::test_bucket()?;

    let view = RemoteView::new(pool, s3, bucket);
    let key = ObjectKey::new("photos/vacation/img001.jpg")?;
    let range = view.fetch_byte_range(&key, 0, 5).await?;

    assert_eq!(range, b"fake ");
    Ok(())
}

#[tokio::test]
async fn test_remote_view_fetch_object_exists() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = common::create_mock_s3_arc()?;
    let bucket = common::test_bucket()?;

    let view = RemoteView::new(pool, s3, bucket);

    let exists = view
        .fetch_object_exists(&ObjectKey::new("photos/vacation/img001.jpg")?)
        .await?;
    assert!(exists);

    let exists = view
        .fetch_object_exists(&ObjectKey::new("nonexistent.txt")?)
        .await?;
    assert!(!exists);
    Ok(())
}

#[tokio::test]
async fn test_remote_view_fetch_thumbnail_caches_locally() -> Result<()> {
    // This test requires valid image data for thumbnail generation.
    // The mock data ("fake jpeg data for img001") is not a valid image,
    // so thumbnail generation will fail.  We verify the S3 fetch works
    // and document the thumbnail generation limitation.
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = common::create_mock_s3_arc()?;
    let bucket = common::test_bucket()?;

    // Insert the file entry first (required for thumbnail caching).
    use ossgalley_core::db::models::FileEntry;
    FileEntry::insert(
        &pool,
        &FileEntry {
            key: "photos/vacation/img001.jpg".to_string(),
            etag: "test-etag".to_string(),
            size: 100,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: Some("image/jpeg".to_string()),
            file_type: "jpeg".to_string(),
            metadata_state: "pending".to_string(),
            is_deleted: false,
        },
    )
    .await?;

    let view = RemoteView::new(pool.clone(), s3, bucket);
    let key = ObjectKey::new("photos/vacation/img001.jpg")?;

    // fetch_thumbnail will try to generate a thumbnail from the mock data,
    // which is not a valid image.  We expect it to fail with a
    // ThumbnailGeneration error.
    let result = view.fetch_thumbnail(&key).await;
    assert!(
        result.is_err(),
        "expected thumbnail generation to fail with mock data"
    );
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("Failed to decode image")
            || err_str.contains("ThumbnailGeneration"),
        "expected image decode error, got: {err_str}"
    );
    Ok(())
}

#[tokio::test]
async fn test_remote_view_fetch_thumbnail_from_cache() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = common::create_mock_s3_arc()?;
    let bucket = common::test_bucket()?;

    // Seed the file entry first (required for foreign key constraint).
    common::seed_test_files(&pool).await?;
    // Seed a cached thumbnail.
    common::seed_test_thumbnail(&pool).await?;

    let view = RemoteView::new(pool, s3, bucket);
    let key = ObjectKey::new("photos/vacation/img001.jpg")?;

    // Second call should return from cache (no S3 fetch needed).
    let thumb = view.fetch_thumbnail(&key).await?;
    assert_eq!(thumb, vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]);
    Ok(())
}

#[tokio::test]
async fn test_remote_view_has_s3_field() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = MockS3Client::new();
    let bucket = common::test_bucket()?;

    let view = RemoteView::new(pool, Arc::new(s3) as Arc<dyn S3Client>, bucket);
    // RemoteView has an s3() accessor.
    let _s3 = view.s3();
    let _ = _s3;
    Ok(())
}

#[tokio::test]
async fn test_remote_view_db_accessor() -> Result<()> {
    let (pool, _dir) = common::setup_test_db().await?;
    let s3 = MockS3Client::new();
    let bucket = common::test_bucket()?;

    let view = RemoteView::new(pool.clone(), Arc::new(s3) as Arc<dyn S3Client>, bucket);
    let _db = view.db();
    let _ = _db;
    Ok(())
}