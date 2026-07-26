//! ProcessLayer — orchestrates EXIF extraction and tag parsing.
//!
//! Calls inner, queries pending files, then runs two-phase batch processing:
//! 1. `BatchService<ExifService>`: concurrent download + EXIF extraction
//! 2. `BatchService<TagService>`: concurrent tag parsing

use std::sync::Arc;

use tokio::sync::Mutex;
use tower::service_fn;
use tower::util::BoxService;
use tower::{Layer, Service};

use crate::classify::classifier::{classify_extension, parse_extension};
use crate::error::S3GalleryError;
use crate::scan::batch_service::BatchService;
use crate::scan::exif_service::ExifService;
use crate::scan::pipeline::{
    ExifRequest, ExifResult, HostProcessResult, ScanRequest, ScanResponse, TagRequest, TagResponse,
};
use crate::scan::tag_service::TagService;
use crate::types::ObjectKey;

/// Raw DB row type from sqlx queries listing files.
type FileRow = (String, String);

/// ProcessLayer wraps an inner service with metadata extraction.
pub struct ProcessLayer {
    db: sqlx::SqlitePool,
    batch_exif: BatchService<ExifService, ExifRequest, ExifResult>,
    batch_tag: BatchService<TagService, TagRequest, TagResponse>,
}

impl ProcessLayer {
    pub fn new(
        db: sqlx::SqlitePool,
        exif_s3: crate::s3::s3_service::S3Service,
        concurrency: usize,
    ) -> Self {
        let exif_service = ExifService::new(exif_s3, db.clone());
        let tag_service = TagService::new(db.clone());
        Self {
            db,
            batch_exif: BatchService::new(exif_service, concurrency),
            batch_tag: BatchService::new(tag_service, concurrency),
        }
    }
}

impl<I> Layer<I> for ProcessLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError> + Send + 'static,
    I::Future: Send,
{
    type Service = BoxService<ScanRequest, ScanResponse, S3GalleryError>;

    fn layer(&self, inner: I) -> Self::Service {
        let db = self.db.clone();
        let batch_exif = self.batch_exif.clone();
        let batch_tag = self.batch_tag.clone();
        let inner = Arc::new(Mutex::new(inner));

        BoxService::new(service_fn(move |req: ScanRequest| {
            let db = db.clone();
            let inner = inner.clone();
            let mut batch_exif = batch_exif.clone();
            let mut batch_tag = batch_tag.clone();
            let extract_metadata = req.extract_metadata;
            let bucket = req.bucket.clone();

            async move {
                // 1. Call inner (DiffLayer -> DiscoverLayer)
                let mut resp = {
                    let mut inner = inner.lock().await;
                    inner.call(req).await?
                };

                // 2. Process pending files for each host
                let mut process_results = Vec::new();
                for host in &resp.hosts {
                    if !extract_metadata {
                        process_results.push(HostProcessResult {
                            host_id: host.host_id.clone(),
                            processed_count: 0,
                            failed_count: 0,
                        });
                        continue;
                    }

                    // Find pending files
                    let pending: Vec<FileRow> = sqlx::query_as(
                        "SELECT key, file_type FROM files WHERE host_id = ? AND metadata_state = 'pending' AND is_deleted = 0",
                    )
                    .bind(&host.host_id)
                    .fetch_all(&db)
                    .await
                    .map_err(|e| S3GalleryError::DbError(format!("Failed to query pending files: {e}")))?;

                    // Phase 1: Build ExifRequests with context tracking
                    struct ExifContext {
                        key: String,
                        file_type: String,
                        host_id: String,
                    }

                    let mut contexts = Vec::new();
                    let mut exif_reqs = Vec::new();

                    for entry in &pending {
                        let key_str = entry.0.as_str();
                        let file_name = key_str.rsplit('/').next().unwrap_or(key_str);
                        if let Some(ext) = parse_extension(file_name) {
                            let file_type = classify_extension(&ext).to_string();
                            if let Ok(key) = ObjectKey::new(entry.0.clone()) {
                                contexts.push(ExifContext {
                                    key: entry.0.clone(),
                                    file_type: file_type.clone(),
                                    host_id: host.host_id.clone(),
                                });
                                exif_reqs.push(ExifRequest {
                                    bucket: bucket.clone(),
                                    key,
                                    host_id: host.host_id.clone(),
                                    file_type,
                                    ext: ext.to_string(),
                                });
                            }
                        }
                    }

                    // Phase 1: Batch EXIF download + extraction
                    let exif_results = batch_exif.call(exif_reqs).await?;

                    // Phase 2: Build TagRequests from successful EXIF results
                    let mut tag_reqs = Vec::new();
                    let mut failed = 0u64;

                    for (ctx, result) in contexts.into_iter().zip(exif_results) {
                        match result {
                            Ok(ExifResult::Some(data)) => {
                                tag_reqs.push(TagRequest {
                                    host_id: ctx.host_id,
                                    key: ctx.key,
                                    exif_data: data,
                                    file_type: ctx.file_type,
                                });
                            }
                            Ok(ExifResult::None) => {
                                // ExifService already set metadata_state = 'extracted'
                            }
                            Err(_) => {
                                failed += 1;
                            }
                        }
                    }

                    // Phase 2: Batch Tag parsing
                    let tag_results = batch_tag.call(tag_reqs).await?;

                    let mut processed = 0u64;
                    for result in tag_results {
                        match result {
                            Ok(_) => {
                                processed += 1;
                            }
                            Err(_) => {
                                failed += 1;
                            }
                        }
                    }

                    process_results.push(HostProcessResult {
                        host_id: host.host_id.clone(),
                        processed_count: processed,
                        failed_count: failed,
                    });
                }

                resp.process_results = process_results;
                Ok(resp)
            }
        }))
    }
}
