//! Directory listing functionality.

use std::collections::HashSet;
use std::str::FromStr;

use sqlx::SqlitePool;

use crate::db::models::DirSizeEntry;
use crate::db::models::FileEntry;
use crate::error::Result;
use crate::types::{FileSize, FileType, SortField, SortOrder};

/// A single entry in a directory listing.
#[derive(Debug, Clone)]
pub struct LsEntry {
    /// Name of the file or directory.
    pub name: String,
    /// File type (only valid for files).
    pub file_type: FileType,
    /// File size (0 for directories that haven't been computed).
    pub size: FileSize,
    /// Number of files in this directory (0 for files).
    pub file_count: u64,
    /// Last modified timestamp (ISO-8601).
    pub last_modified: String,
    /// Whether this is a directory.
    pub is_directory: bool,
}

impl LsEntry {
    fn directory(name: String) -> Self {
        Self {
            name,
            file_type: FileType::Unknown,
            size: FileSize::new(0),
            file_count: 0,
            last_modified: String::new(),
            is_directory: true,
        }
    }

    fn from_file(file: &FileEntry, name: String) -> Result<Self> {
        let file_type = FileType::from_str(&file.file_type).unwrap_or(FileType::Unknown);
        let size = u64::try_from(file.size).unwrap_or(0);
        Ok(Self {
            name,
            file_type,
            size: FileSize::new(size),
            file_count: 0,
            last_modified: file.last_modified.clone(),
            is_directory: false,
        })
    }
}

/// List files in a directory prefix.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn list_directory(
    db: &SqlitePool,
    host_id: &str,
    prefix: &str,
    sort_by: SortField,
    sort_order: SortOrder,
) -> Result<Vec<LsEntry>> {
    let files = FileEntry::list_by_prefix(db, host_id, prefix).await?;

    let effective_prefix = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    };

    let mut directories = HashSet::new();
    let mut file_entries = Vec::new();

    for file in files {
        let key = &file.key;
        if !key.starts_with(&effective_prefix) {
            continue;
        }

        let remaining = &key[effective_prefix.len()..];
        if let Some(slash_pos) = remaining.find('/') {
            let dir_name = &remaining[..slash_pos];
            directories.insert(dir_name.to_string());
        } else {
            if remaining.is_empty() {
                continue;
            }
            let name = remaining.to_string();
            file_entries.push((file, name));
        }
    }

    // Fetch directory sizes from dir_sizes table
    let dir_sizes = DirSizeEntry::list_by_prefix(db, host_id, prefix)
        .await
        .unwrap_or_default();
    let size_map: std::collections::HashMap<String, (i64, i64)> = dir_sizes
        .iter()
        .map(|d| (d.dir_path.clone(), (d.total_size, d.total_files)))
        .collect();

    let mut entries = Vec::new();

    for dir in directories {
        let dir_path = format!("{}{}/", effective_prefix, dir);
        let (size, count) = size_map.get(&dir_path).copied().unwrap_or((0, 0));
        let mut entry = LsEntry::directory(dir);
        entry.size = FileSize::new(size as u64);
        entry.file_count = count as u64;
        entries.push(entry);
    }

    for (file, name) in file_entries {
        entries.push(LsEntry::from_file(&file, name)?);
    }

    sort_entries(&mut entries, sort_by, sort_order);

    Ok(entries)
}

fn sort_entries(entries: &mut [LsEntry], sort_by: SortField, sort_order: SortOrder) {
    entries.sort_by(|a, b| {
        let dir_cmp = a.is_directory.cmp(&b.is_directory).reverse();
        if dir_cmp != std::cmp::Ordering::Equal {
            return apply_sort_order(dir_cmp, sort_order);
        }

        let cmp = match sort_by {
            SortField::Name => a.name.cmp(&b.name),
            SortField::Size => a.size.as_u64().cmp(&b.size.as_u64()),
            SortField::LastModified => a.last_modified.cmp(&b.last_modified),
            SortField::FileType => a.file_type.to_string().cmp(&b.file_type.to_string()),
        };

        apply_sort_order(cmp, sort_order)
    });
}

fn apply_sort_order(cmp: std::cmp::Ordering, sort_order: SortOrder) -> std::cmp::Ordering {
    match sort_order {
        SortOrder::Ascending => cmp,
        SortOrder::Descending => cmp.reverse(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::FileEntry;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    async fn setup_test_db() -> Result<(SqlitePool, tempfile::TempDir)> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;
        Ok((pool, dir))
    }

    async fn seed_test_files(pool: &SqlitePool) -> Result<()> {
        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "test-host".to_string(),
                key: "photos/2024/img001.jpg".to_string(),
                etag: "\"abc123\"".to_string(),
                size: 1024,
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "test-host".to_string(),
                key: "photos/2024/img002.jpg".to_string(),
                etag: "\"def456\"".to_string(),
                size: 2048,
                last_modified: "2024-01-02T00:00:00Z".to_string(),
                content_type: Some("image/jpeg".to_string()),
                file_type: "jpeg".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "test-host".to_string(),
                key: "videos/clip.mp4".to_string(),
                etag: "\"ghi789\"".to_string(),
                size: 50000,
                last_modified: "2024-02-01T00:00:00Z".to_string(),
                content_type: Some("video/mp4".to_string()),
                file_type: "mp4".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: false,
            },
        )
        .await?;

        FileEntry::upsert(
            pool,
            &FileEntry {
                host_id: "test-host".to_string(),
                key: "docs/notes.txt".to_string(),
                etag: "\"jkl012\"".to_string(),
                size: 50,
                last_modified: "2024-03-01T00:00:00Z".to_string(),
                content_type: None,
                file_type: "unknown".to_string(),
                metadata_state: "pending".to_string(),
                effective_date: "".to_string(),
                is_deleted: true,
            },
        )
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_list_directory_root() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let entries = list_directory(
            &pool,
            "test-host",
            "",
            SortField::Name,
            SortOrder::Ascending,
        )
        .await?;
        assert_eq!(entries.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_list_directory_photos_2024() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let entries = list_directory(
            &pool,
            "test-host",
            "photos/2024",
            SortField::Name,
            SortOrder::Ascending,
        )
        .await?;
        assert_eq!(entries.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_list_directory_deleted_excluded() -> Result<()> {
        let (pool, _dir) = setup_test_db().await?;
        seed_test_files(&pool).await?;

        let entries = list_directory(
            &pool,
            "test-host",
            "docs",
            SortField::Name,
            SortOrder::Ascending,
        )
        .await?;
        assert!(entries.is_empty());

        Ok(())
    }
}
