//! DiffLayer -- compares scan_objects against files table, updates files table.
//!
//! This is the second layer, generic over inner service.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use sqlx::SqlitePool;
use tower::{Layer, Service};

use crate::classify::classifier::{classify_extension, content_type_from_extension, parse_extension};
use crate::error::Result;
use crate::s3::client::ObjectSummary;
use crate::types::{Etag, FileSize, ObjectKey};
use crate::scan::diff::{apply_diff, diff_objects};
use crate::scan::pipeline::{HostDiffResult, ScanRequest, ScanResponse};
use crate::scan::scan_objects::ScanObjectEntry;
use crate::types::FileType;

/// DiffLayer wraps an inner service with DB diff logic.
pub struct DiffLayer {
    db: SqlitePool,
}

impl DiffLayer {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl<I> Layer<I> for DiffLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = crate::error::S3GalleryError>
        + Clone
        + Send
        + 'static,
    I::Future: Send,
{
    type Service = DiffService<I>;

    fn layer(&self, inner: I) -> Self::Service {
        DiffService {
            inner,
            db: self.db.clone(),
        }
    }
}

/// DiffService -- calls inner, then diffs scan_objects against files table.
#[derive(Clone)]
pub struct DiffService<I> {
    inner: I,
    db: SqlitePool,
}

impl<I> Service<ScanRequest> for DiffService<I>
where
    I: Service<ScanRequest, Response = ScanResponse, Error = crate::error::S3GalleryError>
        + Clone
        + Send
        + 'static,
    I::Future: Send,
{
    type Response = ScanResponse;
    type Error = crate::error::S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ScanRequest) -> Self::Future {
        let mut inner = self.inner.clone();
        let db = self.db.clone();

        Box::pin(async move {
            // 1. Call inner (DiscoverLayer)
            let mut resp = inner.call(req).await?;

            // 2. For each host, diff scan_objects against files table
            let mut diff_results = Vec::new();
            for host in &resp.hosts {
                let scan_entries = ScanObjectEntry::list_by_scan(
                    &db, &resp.scan_id, &host.host_id,
                )
                .await?;

                // Convert to ObjectSummary for diff
                let s3_objects: Vec<ObjectSummary> = scan_entries
                    .iter()
                    .map(|entry| {
                        // Safety: DB data was validated on insert, so new() always succeeds
                        ObjectSummary {
                            key: ObjectKey::new(entry.key.clone())
                                .expect("valid key from DB"),
                            etag: Etag::new(entry.etag.clone())
                                .expect("valid etag from DB"),
                            size: FileSize::new(entry.size as u64),
                            last_modified: entry.last_modified.clone(),
                        }
                    })
                    .collect();

                // Get existing DB entries
                let db_entries = crate::db::models::FileEntry::list_by_prefix(
                    &db, &host.host_id, host.prefix.as_str(),
                )
                .await?;

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
        })
    }
}