//! Directory tree functionality.

use std::collections::HashMap;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, Order, QueryFilter, QueryOrder};

use crate::entity::file;
use crate::error::Result;
use crate::error::S3GalleryError;
use crate::types::FileSize;

/// A node in the directory tree.
#[derive(Debug, Clone)]
pub struct TreeNode {
    /// Name of this node (file or directory name).
    pub name: String,
    /// Full path of this node.
    pub path: String,
    /// Whether this is a directory.
    pub is_directory: bool,
    /// Child nodes (only for directories).
    pub children: Vec<TreeNode>,
    /// Number of files in this subtree.
    pub file_count: u64,
    /// Total size of files in this subtree.
    pub total_size: FileSize,
}

/// Build a directory tree.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn build_tree(
    db: &DatabaseConnection,
    host_id: &str,
    root_prefix: &str,
) -> Result<TreeNode> {
    let files = file::Entity::find()
        .filter(file::Column::HostId.eq(host_id))
        .filter(file::Column::IsDeleted.eq(false))
        .filter(file::Column::Key.starts_with(root_prefix))
        .order_by(file::Column::Key, Order::Asc)
        .all(db)
        .await
        .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

    let effective_prefix = if root_prefix.is_empty() {
        String::new()
    } else {
        format!("{root_prefix}/")
    };

    #[derive(Debug, Clone)]
    struct NodeData {
        name: String,
        is_directory: bool,
        children: Vec<String>,
        file_count: u64,
        total_size: u64,
    }

    let mut node_map: HashMap<String, NodeData> = HashMap::new();

    let root_path = root_prefix.to_string();

    let root_name = if root_prefix.is_empty() {
        "".to_string()
    } else {
        root_prefix.to_string()
    };

    node_map.insert(
        root_path.clone(),
        NodeData {
            name: root_name,
            is_directory: true,
            children: Vec::new(),
            file_count: 0,
            total_size: 0,
        },
    );

    for file in &files {
        let key_str = file.key.as_str();
        if !key_str.starts_with(&effective_prefix) && !key_str.is_empty() {
            continue;
        }

        let remaining = if key_str.starts_with(&effective_prefix) {
            &key_str[effective_prefix.len()..]
        } else {
            key_str
        };

        if remaining.is_empty() {
            continue;
        }

        let parts: Vec<&str> = remaining.split('/').collect();
        let parts_len = parts.len();

        let mut parent_path = root_path.clone();

        for (i, &part) in parts.iter().enumerate() {
            let is_last = i == parts_len - 1;
            let new_path = if parent_path.is_empty() {
                part.to_string()
            } else {
                format!("{parent_path}/{part}")
            };

            if !node_map.contains_key(&new_path) {
                node_map.insert(
                    new_path.clone(),
                    NodeData {
                        name: part.to_string(),
                        is_directory: !is_last,
                        children: Vec::new(),
                        file_count: 0,
                        total_size: 0,
                    },
                );
            }

            if let Some(parent) = node_map.get_mut(&parent_path) {
                if !parent.children.contains(&new_path) {
                    parent.children.push(new_path.clone());
                }
            }

            parent_path = new_path;
        }

        if let Some(node) = node_map.get_mut(key_str) {
            node.is_directory = false;
            let size = file.size.as_u64();
            node.total_size = size;
            node.file_count = 1;
        }

        let file_size = file.size.as_u64();

        if let Some(root_node) = node_map.get_mut(&root_path) {
            root_node.total_size += file_size;
            root_node.file_count += 1;
        }

        let mut parent_path = root_path.clone();

        for (i, &part) in parts.iter().enumerate() {
            if i == parts_len - 1 {
                break;
            }

            let new_path = if parent_path.is_empty() {
                part.to_string()
            } else {
                format!("{parent_path}/{part}")
            };

            if let Some(node) = node_map.get_mut(&new_path) {
                node.total_size += file_size;
                node.file_count += 1;
            }

            parent_path = new_path;
        }
    }

    let mut dir_info: HashMap<String, bool> = HashMap::new();
    for (path, node) in &node_map {
        dir_info.insert(path.clone(), node.is_directory);
    }

    for node in node_map.values_mut() {
        node.children.sort_by(|a, b| {
            let a_is_dir = dir_info.get(a).copied().unwrap_or(false);
            let b_is_dir = dir_info.get(b).copied().unwrap_or(false);
            let dir_cmp = b_is_dir.cmp(&a_is_dir);
            if dir_cmp != std::cmp::Ordering::Equal {
                return dir_cmp;
            }
            a.cmp(b)
        });
    }

    fn build_node(path: &str, node_map: &HashMap<String, NodeData>) -> Result<TreeNode> {
        let data = node_map
            .get(path)
            .ok_or_else(|| crate::error::S3GalleryError::Internal("Node not found".into()))?;

        let mut children = Vec::new();
        for child_path in &data.children {
            children.push(build_node(child_path, node_map)?);
        }

        Ok(TreeNode {
            name: data.name.clone(),
            path: path.to_string(),
            is_directory: data.is_directory,
            children,
            file_count: data.file_count,
            total_size: FileSize::new(data.total_size),
        })
    }

    build_node(&root_path, &node_map)
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

        Ok(())
    }

    #[tokio::test]
    async fn test_build_tree() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let tree = build_tree(&db, "test-host", "").await?;

        assert!(tree.is_directory);
        assert_eq!(tree.file_count, 3);
        assert_eq!(tree.total_size.as_u64(), 1024 + 2048 + 50000);
        assert_eq!(tree.children.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_build_tree_subdir() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;
        seed_test_files(&db).await?;

        let tree = build_tree(&db, "test-host", "photos").await?;

        assert!(tree.is_directory);
        assert_eq!(tree.file_count, 2);
        assert_eq!(tree.total_size.as_u64(), 1024 + 2048);

        Ok(())
    }

    #[tokio::test]
    async fn test_build_tree_empty() -> Result<()> {
        let (db, _dir) = setup_test_db().await?;

        let tree = build_tree(&db, "test-host", "").await?;

        assert!(tree.is_directory);
        assert_eq!(tree.file_count, 0);
        assert_eq!(tree.total_size.as_u64(), 0);
        assert!(tree.children.is_empty());

        Ok(())
    }
}
