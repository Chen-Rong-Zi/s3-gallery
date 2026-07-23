//! ProcessLayer — extracts metadata for pending files.
//!
//! This is the third pipeline layer, generic over inner service.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use chrono::Utc;
use tower::{Layer, Service};

use crate::db::models::{FileEntry, MetadataEntry, TagEntry, FileTagEntry};
use crate::error::{Result, S3GalleryError};
use crate::extractor::exif::ExifExtractor;
use crate::extractor::registry::ExtractorRegistry;
use crate::extractor::tag_rules::{evaluate_all, TagRule};
use crate::s3::s3_service::S3Service;
use crate::scan::pipeline::{HostProcessResult, ScanRequest, ScanResponse};
use crate::classify::classifier::parse_extension;
use crate::types::ObjectKey;

/// ProcessLayer wraps an inner service with metadata extraction.
pub struct ProcessLayer {
    db: sqlx::SqlitePool,
    exif_s3: S3Service,
}

impl ProcessLayer {
    pub fn new(db: sqlx::SqlitePool, exif_s3: S3Service) -> Self {
        Self { db, exif_s3 }
    }
}

impl<I> Layer<I> for ProcessLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = crate::error::S3GalleryError>
        + Clone
        + Send
        + 'static,
    I::Future: Send,
{
    type Service = ProcessService<I>;

    fn layer(&self, inner: I) -> Self::Service {
        ProcessService {
            inner,
            db: self.db.clone(),
            exif_s3: self.exif_s3.clone(),
        }
    }
}

/// ProcessService — calls inner, then extracts metadata for pending files.
#[derive(Clone)]
pub struct ProcessService<I> {
    inner: I,
    db: sqlx::SqlitePool,
    exif_s3: S3Service,
}

impl<I> Service<ScanRequest> for ProcessService<I>
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
        let mut exif_s3 = self.exif_s3.clone();
        let extract_metadata = req.extract_metadata;
        let bucket = req.bucket.clone();

        Box::pin(async move {
            // 1. Call inner (DiffLayer -> DiscoverLayer)
            let mut resp = inner.call(req).await?;

            // 2. Extract metadata for pending files if enabled
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

                // Find pending files for this host
                let pending: Vec<FileEntry> = sqlx::query_as(
                    "SELECT * FROM files WHERE host_id = ? AND metadata_state = 'pending' AND is_deleted = 0",
                )
                .bind(&host.host_id)
                .fetch_all(&db)
                .await
                .map_err(|e| S3GalleryError::DbError(format!("Failed to query pending files: {e}")))?;

                let mut registry = ExtractorRegistry::new();
                registry.register(Box::new(ExifExtractor::new()));
                let tag_rules = TagRule::default_rules();

                let mut processed = 0u64;
                let mut failed = 0u64;

                for entry in &pending {
                    let key_str = entry.key.as_str();
                    let file_name = key_str.rsplit('/').next().unwrap_or(key_str);
                    let ext = match parse_extension(file_name) {
                        Some(e) => e,
                        None => continue,
                    };

                    // Check if any extractor supports this file type
                    let file_type = crate::classify::classifier::classify_extension(&ext).to_string();
                    if registry.find(&file_type, ext.as_str()).is_empty() {
                        // Mark as extracted so we don't retry
                        let _ = sqlx::query(
                            "UPDATE files SET metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
                        )
                        .bind(&host.host_id)
                        .bind(key_str)
                        .execute(&db)
                        .await;
                        continue;
                    }

                    // Download 64KB for EXIF
                    let obj_key = match ObjectKey::new(key_str.to_string()) {
                        Ok(k) => k,
                        Err(_) => continue,
                    };

                    let data = match exif_s3
                        .get_object_range(&bucket, &obj_key, 0, 65536)
                        .await
                    {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!(key = %key_str, error = %e, "failed to download range for metadata");
                            let _ = sqlx::query(
                                "UPDATE files SET metadata_state = 'failed' WHERE host_id = ? AND key = ?",
                            )
                            .bind(&host.host_id)
                            .bind(key_str)
                            .execute(&db)
                            .await;
                            failed += 1;
                            continue;
                        }
                    };

                    // Extract metadata
                    let items = match registry.extract_all(&data, &file_type, ext.as_str()).await {
                        Ok(items) => items,
                        Err(e) => {
                            tracing::warn!(key = %key_str, error = %e, "metadata extraction failed");
                            let _ = sqlx::query(
                                "UPDATE files SET metadata_state = 'failed' WHERE host_id = ? AND key = ?",
                            )
                            .bind(&host.host_id)
                            .bind(key_str)
                            .execute(&db)
                            .await;
                            failed += 1;
                            continue;
                        }
                    };

                    if items.is_empty() {
                        let _ = sqlx::query(
                            "UPDATE files SET metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
                        )
                        .bind(&host.host_id)
                        .bind(key_str)
                        .execute(&db)
                        .await;
                        continue;
                    }

                    // Store metadata
                    let now = Utc::now().to_rfc3339();
                    for item in &items {
                        MetadataEntry::insert(&db, &MetadataEntry {
                            file_key: key_str.to_string(),
                            namespace: item.namespace.to_string(),
                            key: item.key.clone(),
                            value: item.value.clone(),
                            extracted_at: now.clone(),
                            partial: false,
                        }).await?;
                    }

                    // Generate and store tags
                    let tags = evaluate_all(&tag_rules, &items, &file_type);
                    for tag in &tags {
                        let tag_id = TagEntry::ensure_exists(&db, &tag.tag_name, &tag.tag_type).await?;
                        FileTagEntry::insert(&db, &FileTagEntry {
                            file_key: key_str.to_string(),
                            tag_id,
                        }).await?;
                    }

                    // Add exif:yes tag
                    let exif_tag_id = TagEntry::ensure_exists(&db, "exif:yes", "auto").await?;
                    FileTagEntry::insert(&db, &FileTagEntry {
                        file_key: key_str.to_string(),
                        tag_id: exif_tag_id,
                    }).await?;

                    // Update effective_date
                    let exif_date = items
                        .iter()
                        .find(|m| m.key == "DateTimeOriginal" || m.key == "DateTimeDigitized")
                        .map(|m| m.value.as_str())
                        .and_then(|v| v.get(..10))
                        .map(|d| d.replace(":", "-"));
                    let effective_date = exif_date
                        .as_deref()
                        .unwrap_or_else(|| &entry.last_modified[..10.min(entry.last_modified.len())]).to_string();

                    // SAFETY: effective_date is safe to index because we bound
                    // the range to the string length via 10.min(len).
                    sqlx::query(
                        "UPDATE files SET effective_date = ?, metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
                    )
                    .bind(&effective_date)
                    .bind(&host.host_id)
                    .bind(key_str)
                    .execute(&db)
                    .await
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

                    processed += 1;
                }

                process_results.push(HostProcessResult {
                    host_id: host.host_id.clone(),
                    processed_count: processed,
                    failed_count: failed,
                });
            }

            resp.process_results = process_results;
            Ok(resp)
        })
    }
}