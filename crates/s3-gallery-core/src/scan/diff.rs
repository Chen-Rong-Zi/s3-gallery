use crate::db::models::FileEntry;
use crate::s3::client::ObjectSummary;
use std::collections::HashMap;

/// Result of diffing S3 listing against DB state
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// Objects that are new (not in DB)
    pub new_objects: Vec<ObjectSummary>,
    /// Objects whose ETag has changed
    pub changed_objects: Vec<ObjectSummary>,
    /// Keys that were in DB but not in S3 (deleted from OSS)
    pub deleted_keys: Vec<String>,
    /// Objects that are unchanged (same ETag)
    pub unchanged_count: u64,
}

/// Compare S3 objects against DB file entries to find differences.
///
/// Takes an S3 object listing and existing DB entries and computes:
/// - New objects: in S3 but not in DB
/// - Changed objects: in both but with different ETag
/// - Deleted keys: in DB but not in S3
/// - Unchanged count: in both with same ETag
pub fn diff_objects(s3_objects: &[ObjectSummary], db_entries: &[FileEntry]) -> DiffResult {
    let mut db_map: HashMap<&str, &str> = HashMap::new();
    for entry in db_entries {
        if !entry.is_deleted {
            db_map.insert(entry.key.as_str(), entry.etag.as_str());
        }
    }

    let mut seen_keys = HashMap::new();
    let mut new_objects = Vec::new();
    let mut changed_objects = Vec::new();
    let mut unchanged_count = 0u64;

    for obj in s3_objects {
        let key_str = obj.key.as_str();
        seen_keys.insert(key_str, true);

        match db_map.get(key_str) {
            Some(db_etag) if *db_etag == obj.etag.as_str() => {
                unchanged_count += 1;
            }
            Some(_) => {
                changed_objects.push(obj.clone());
            }
            None => {
                new_objects.push(obj.clone());
            }
        }
    }

    // Find deleted keys (in DB but not in S3)
    let deleted_keys: Vec<String> = db_map
        .keys()
        .filter(|k| !seen_keys.contains_key(*k))
        .map(|k| k.to_string())
        .collect();

    DiffResult {
        new_objects,
        changed_objects,
        deleted_keys,
        unchanged_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::s3::client::ObjectSummary;
    use crate::types::{Etag, FileSize, ObjectKey};

    fn make_file_entry(key: &str, etag: &str) -> FileEntry {
        FileEntry {
            host_id: "test-host".to_string(),
            key: key.to_string(),
            etag: etag.to_string(),
            size: 100,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            content_type: None,
            file_type: "unknown".to_string(),
            metadata_state: "pending".to_string(),
            effective_date: "".to_string(),
            is_deleted: false,
        }
    }

    #[tokio::test]
    async fn test_diff_objects_all_new() -> Result<()> {
        let s3_objects = vec![
            ObjectSummary {
                key: ObjectKey::new("a.jpg")?,
                etag: Etag::new("e1")?,
                size: FileSize::new(100),
                last_modified: "now".to_string(),
            },
            ObjectSummary {
                key: ObjectKey::new("b.jpg")?,
                etag: Etag::new("e2")?,
                size: FileSize::new(200),
                last_modified: "now".to_string(),
            },
        ];
        let db_entries = vec![];

        let result = diff_objects(&s3_objects, &db_entries);
        assert_eq!(result.new_objects.len(), 2);
        assert_eq!(result.changed_objects.len(), 0);
        assert_eq!(result.deleted_keys.len(), 0);
        assert_eq!(result.unchanged_count, 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_diff_objects_all_unchanged() -> Result<()> {
        let s3_objects = vec![ObjectSummary {
            key: ObjectKey::new("a.jpg")?,
            etag: Etag::new("e1")?,
            size: FileSize::new(100),
            last_modified: "now".to_string(),
        }];
        let db_entries = vec![make_file_entry("a.jpg", "e1")];

        let result = diff_objects(&s3_objects, &db_entries);
        assert_eq!(result.new_objects.len(), 0);
        assert_eq!(result.changed_objects.len(), 0);
        assert_eq!(result.deleted_keys.len(), 0);
        assert_eq!(result.unchanged_count, 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_diff_objects_changed() -> Result<()> {
        let s3_objects = vec![ObjectSummary {
            key: ObjectKey::new("a.jpg")?,
            etag: Etag::new("e2")?, // Different from DB
            size: FileSize::new(100),
            last_modified: "now".to_string(),
        }];
        let db_entries = vec![make_file_entry("a.jpg", "e1")];

        let result = diff_objects(&s3_objects, &db_entries);
        assert_eq!(result.new_objects.len(), 0);
        assert_eq!(result.changed_objects.len(), 1);
        assert_eq!(result.deleted_keys.len(), 0);
        assert_eq!(result.unchanged_count, 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_diff_objects_deleted() -> Result<()> {
        let s3_objects = vec![];
        let db_entries = vec![make_file_entry("a.jpg", "e1")];

        let result = diff_objects(&s3_objects, &db_entries);
        assert_eq!(result.new_objects.len(), 0);
        assert_eq!(result.changed_objects.len(), 0);
        assert_eq!(result.deleted_keys.len(), 1);
        assert_eq!(result.deleted_keys[0], "a.jpg");
        assert_eq!(result.unchanged_count, 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_diff_objects_mixed() -> Result<()> {
        let s3_objects = vec![
            ObjectSummary {
                key: ObjectKey::new("new.jpg")?, // New
                etag: Etag::new("e1")?,
                size: FileSize::new(100),
                last_modified: "now".to_string(),
            },
            ObjectSummary {
                key: ObjectKey::new("changed.jpg")?, // Changed
                etag: Etag::new("e2-new")?,
                size: FileSize::new(100),
                last_modified: "now".to_string(),
            },
            ObjectSummary {
                key: ObjectKey::new("unchanged.jpg")?, // Unchanged
                etag: Etag::new("e3")?,
                size: FileSize::new(100),
                last_modified: "now".to_string(),
            },
        ];
        let db_entries = vec![
            make_file_entry("changed.jpg", "e2-old"),
            make_file_entry("unchanged.jpg", "e3"),
            make_file_entry("deleted.jpg", "e4"), // Deleted
        ];

        let result = diff_objects(&s3_objects, &db_entries);
        assert_eq!(result.new_objects.len(), 1);
        assert_eq!(result.changed_objects.len(), 1);
        assert_eq!(result.deleted_keys.len(), 1);
        assert_eq!(result.unchanged_count, 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_diff_ignores_deleted_entries() -> Result<()> {
        let s3_objects = vec![];
        let mut deleted_entry = make_file_entry("deleted-in-db.jpg", "e1");
        deleted_entry.is_deleted = true;

        let result = diff_objects(&s3_objects, &[deleted_entry]);
        assert_eq!(result.deleted_keys.len(), 0); // Already marked as deleted
        Ok(())
    }
}
