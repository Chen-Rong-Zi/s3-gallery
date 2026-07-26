//! DiffLayer -- compares scan_objects against files table, updates files table.
//!
//! This is the second layer, generic over inner service.

use std::collections::HashMap;
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use sqlx::SqlitePool;
use tokio::sync::Mutex;
use tower::service_fn;
use tower::util::BoxService;
use tower::{Layer, Service};

use crate::classify::classifier::{
    classify_extension, content_type_from_extension, parse_extension,
};
use crate::entity::file::Model as FileEntry;
use crate::s3::client::ObjectSummary;
use crate::scan::diff::{apply_diff, diff_objects};
use crate::scan::pipeline::{HostDiffResult, ScanRequest, ScanResponse};
use crate::types::{Etag, FileSize, HostId, MetadataState, ObjectKey};

/// Raw DB row type from sqlx queries listing files.
type FileRow = (
    String,
    String,
    String,
    i64,
    String,
    Option<String>,
    String,
    String,
    String,
    bool,
);
use crate::scan::scan_objects::ScanObjectEntry;
use crate::types::FileType;

/// DiffLayer wraps an inner service with DB diff logic.
pub struct DiffLayer {
    db: SqlitePool,
    sea_db: DatabaseConnection,
}

impl DiffLayer {
    pub fn new(db: SqlitePool, sea_db: DatabaseConnection) -> Self {
        Self { db, sea_db }
    }
}

impl<I> Layer<I> for DiffLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = crate::error::S3GalleryError>
        + Send
        + 'static,
    I::Future: Send,
{
    type Service = BoxService<ScanRequest, ScanResponse, crate::error::S3GalleryError>;

    fn layer(&self, inner: I) -> Self::Service {
        let db = self.db.clone();
        let sea_db = self.sea_db.clone();
        let inner = Arc::new(Mutex::new(inner));
        BoxService::new(service_fn(move |req: ScanRequest| {
            let db = db.clone();
            let sea_db = sea_db.clone();
            let inner = inner.clone();
            async move {
                // 1. Call inner (DiscoverLayer)
                let mut resp = {
                    let mut inner = inner.lock().await;
                    inner.call(req).await?
                };

                // 2. For each host, diff scan_objects against files table
                let mut diff_results = Vec::new();
                for host in &resp.hosts {
                    let scan_entries =
                        ScanObjectEntry::list_by_scan(&sea_db, &resp.scan_id, &host.host_id)
                            .await?;

                    // Convert to ObjectSummary for diff
                    let s3_objects: Vec<ObjectSummary> = scan_entries
                        .iter()
                        .filter_map(|entry| {
                            let key = ObjectKey::new(entry.key.clone()).ok()?;
                            let etag = Etag::new(entry.etag.clone()).ok()?;
                            Some(ObjectSummary {
                                key,
                                etag,
                                size: FileSize::new(entry.size as u64),
                                last_modified: entry.last_modified.clone(),
                            })
                        })
                        .collect();

                    // Get existing DB entries
                    let db_rows: Vec<FileRow> = sqlx::query_as(
                        "SELECT host_id, key, etag, size, last_modified, content_type, file_type, metadata_state, effective_date, is_deleted                          FROM files WHERE host_id = ? AND key LIKE ? || '%' AND is_deleted = 0 ORDER BY key",
                    )
                    .bind(&host.host_id)
                    .bind(host.prefix.as_str())
                    .fetch_all(&db)
                    .await
                    .map_err(|e| crate::error::S3GalleryError::DbError(format!("Failed to list files by prefix: {e}")))?;
                    let mut db_entries: Vec<FileEntry> = Vec::with_capacity(db_rows.len());
                    for (
                        hid,
                        key,
                        etag,
                        size,
                        last_modified,
                        content_type,
                        file_type,
                        metadata_state,
                        effective_date,
                        is_deleted,
                    ) in db_rows
                    {
                        let host_id = HostId::new(hid).map_err(|e| {
                            crate::error::S3GalleryError::DbError(format!("Invalid host_id: {e}"))
                        })?;
                        let key = ObjectKey::new(key).map_err(|e| {
                            crate::error::S3GalleryError::DbError(format!("Invalid key: {e}"))
                        })?;
                        let etag = Etag::new(etag).map_err(|e| {
                            crate::error::S3GalleryError::DbError(format!("Invalid etag: {e}"))
                        })?;
                        let file_type = file_type.parse::<FileType>().map_err(|e| {
                            crate::error::S3GalleryError::DbError(format!("Invalid file_type: {e}"))
                        })?;
                        let metadata_state =
                            metadata_state.parse::<MetadataState>().map_err(|e| {
                                crate::error::S3GalleryError::DbError(format!(
                                    "Invalid metadata_state: {e}"
                                ))
                            })?;
                        db_entries.push(FileEntry {
                            host_id,
                            key,
                            etag,
                            size: FileSize::new(size as u64),
                            last_modified,
                            content_type,
                            file_type,
                            metadata_state,
                            effective_date,
                            is_deleted,
                        });
                    }

                    // Diff
                    let diff = diff_objects(&s3_objects, &db_entries);

                    // Apply diff to files table
                    apply_diff(
                        &db,
                        &host.host_id,
                        &diff,
                        |key| {
                            if let Some(name) = key.file_name() {
                                if let Some(ext) = parse_extension(name) {
                                    return classify_extension(&ext);
                                }
                            }
                            FileType::Unknown
                        },
                        |key| {
                            if let Some(name) = key.file_name() {
                                if let Some(ext) = parse_extension(name) {
                                    return content_type_from_extension(&ext);
                                }
                            }
                            None
                        },
                    )
                    .await?;

                    // Compute file type counts from new+changed objects
                    let mut file_type_counts = HashMap::new();
                    for obj in diff.new_objects.iter().chain(diff.changed_objects.iter()) {
                        let ft = if let Some(name) = obj.key.file_name() {
                            if let Some(ext) = parse_extension(name) {
                                classify_extension(&ext).to_string()
                            } else {
                                "unknown".to_string()
                            }
                        } else {
                            "unknown".to_string()
                        };
                        *file_type_counts.entry(ft).or_insert(0) += 1;
                    }

                    diff_results.push(HostDiffResult {
                        host_id: host.host_id.clone(),
                        new_files: diff.new_objects.len() as u64,
                        changed_files: diff.changed_objects.len() as u64,
                        deleted_files: diff.deleted_keys.len() as u64,
                        unchanged_count: diff.unchanged_count,
                        file_type_counts,
                    });
                }

                resp.diff_results = diff_results;
                Ok(resp)
            }
        }))
    }
}
