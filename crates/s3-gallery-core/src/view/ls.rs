//! Directory listing functionality.

use std::collections::HashSet;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, Order, QueryFilter, QueryOrder};

use crate::entity::dir_size;
use crate::entity::file;
use crate::error::Result;
use crate::error::S3GalleryError;
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

    fn from_file(file: &file::Model, name: String) -> Result<Self> {
        let size = file.size.as_u64();
        Ok(Self {
            name,
            file_type: file.file_type.clone(),
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
    db: &DatabaseConnection,
    host_id: &str,
    prefix: &str,
    sort_by: SortField,
    sort_order: SortOrder,
) -> Result<Vec<LsEntry>> {
    let files = file::Entity::find()
        .filter(file::Column::HostId.eq(host_id))
        .filter(file::Column::IsDeleted.eq(false))
        .filter(file::Column::Key.starts_with(prefix))
        .order_by(file::Column::Key, Order::Asc)
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let effective_prefix = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    };

    let mut directories = HashSet::new();
    let mut file_entries = Vec::new();

    for file in files {
        let key = file.key.as_str();
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
    let dir_sizes = dir_size::Entity::find()
        .filter(dir_size::Column::HostId.eq(host_id))
        .filter(dir_size::Column::DirPath.ne(prefix))
        .filter(dir_size::Column::DirPath.like(format!("{}%", prefix)))
        .order_by(dir_size::Column::DirPath, Order::Asc)
        .all(db)
        .await
        .unwrap_or_default();
    let size_map: std::collections::HashMap<String, (i64, i64)> = dir_sizes
        .iter()
        .map(|d| (d.dir_path.to_string(), (d.total_size, d.total_files)))
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
    use crate::db::migrate::run_full_migration;
    use crate::db::pool::create_pool;
    use crate::error::S3GalleryError;
    use tempfile::tempdir;

    async fn setup_test_db() -> Result<(DatabaseConnection, tempfile::TempDir)> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let db = create_pool(&db_path).await?;
        run_full_migration(&db).await?;
        Ok((db, dir))
    }

    async fn seed_test_files(db: &DatabaseConnection) -> Result<()> {
        let pool = db.get_sqlite_connection_pool();
        sqlx::query(
            "INSERT OR REPLACE INTO files (host_id, key, etag, size, last_modified, content_type, file_type, metadata_state, effective_date, is_deleted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("test-host").bind("photos/2024/img001.jpg").bind("\"abc123\"")
        .bind(1024i64).bind("2024-01-01T00:00:00Z").bind(Some("image/jpeg"))
        .bind("jpeg").bind("pending").bind("").bind(false)
        .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query(
            "INSERT OR REPLACE INTO files (host_id, key, etag, size, last_modified, content_type, file_type, metadata_state, effective_date, is_deleted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("test-host").bind("photos/2024/img002.jpg").bind("\"def456\"")
        .bind(2048i64).bind("2024-01-02T00:00:00Z").bind(Some("image/jpeg"))
        .bind("jpeg").bind("pending").bind("").bind(false)
        .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query(
            "INSERT OR REPLACE INTO files (host_id, key, etag, size, last_modified, content_type, file_type, metadata_state, effective_date, is_deleted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("test-host").bind("videos/clip.mp4").bind("\"ghi789\"")
        .bind(50000i64).bind("2024-02-01T00:00:00Z").bind(Some("video/mp4"))
        .bind("mp4").bind("pending").bind("").bind(false)
        .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        sqlx::query(
            "INSERT OR REPLACE INTO files (host_id, key, etag, size, last_modified, content_type, file_type, metadata_state, effective_date, is_deleted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("test-host").bind("docs/notes.txt").bind("\"jkl012\"")
        .bind(50i64).bind("2024-03-01T00:00:00Z").bind(None::<String>)
        .bind("unknown").bind("pending").bind("").bind(true)
        .execute(pool).await.map_err(|e| S3GalleryError::DbError(e.to_string()))?;

        Ok(())
    }

    #[tokio::test]
    async fn test_list_directory_root() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let entries = list_directory(
            &db,
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
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let entries = list_directory(
            &db,
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
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let entries = list_directory(
            &db,
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
