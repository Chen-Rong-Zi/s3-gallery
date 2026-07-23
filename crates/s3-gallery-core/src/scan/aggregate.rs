//! AggregateLayer — reads DB + TrafficCounters, produces AggregateReport.
//!
//! This is the outermost pipeline layer, generic over inner service.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use sqlx::SqlitePool;
use tokio::sync::Mutex;
use tower::service_fn;
use tower::util::BoxService;
use tower::{Layer, Service};

use crate::error::S3GalleryError;
use crate::s3::traffic_persist::flush_counters;
use crate::s3::traffic_recorder::TrafficCounters;
use crate::scan::pipeline::{
    AggregateReport, ScanRequest, ScanResponse, SizeRanges, TrafficByOperation,
};
use crate::scan::scan_objects::ScanObjectEntry;

/// AggregateLayer wraps an inner service with report generation.
pub struct AggregateLayer {
    db: SqlitePool,
    counters: Arc<TrafficCounters>,
}

impl AggregateLayer {
    /// Create a new `AggregateLayer`.
    pub fn new(db: SqlitePool, counters: Arc<TrafficCounters>) -> Self {
        Self { db, counters }
    }
}

impl<I> Layer<I> for AggregateLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError>
        + Send
        + 'static,
    I::Future: Send,
{
    type Service = BoxService<ScanRequest, ScanResponse, S3GalleryError>;

    fn layer(&self, inner: I) -> Self::Service {
        let db = self.db.clone();
        let counters = self.counters.clone();
        let inner = Arc::new(Mutex::new(inner));
        BoxService::new(service_fn(move |req: ScanRequest| {
            let db = db.clone();
            let counters = counters.clone();
            let inner = inner.clone();
            let start = Instant::now();
            async move {
                // 1. Call inner chain
                let mut resp = {
                    let mut inner = inner.lock().await;
                    inner.call(req).await?
                };

                // 2. Read traffic counters BEFORE flushing (flush resets to 0)
                let mut traffic_by_stage: HashMap<String, HashMap<String, TrafficByOperation>> =
                    HashMap::new();

                let total_download = counters.download_bytes.load(Ordering::Relaxed);
                let total_upload = counters.upload_bytes.load(Ordering::Relaxed);
                let total_requests = counters.request_count.load(Ordering::Relaxed);

                // Build operation-level breakdown under "scan" stage
                let mut scan_stage = HashMap::new();
                for op_idx in 0..counters.per_operation.len() {
                    let bytes = counters.per_operation.get(op_idx).map(|c| c.load(Ordering::Relaxed)).unwrap_or(0);
                    if bytes > 0 {
                        let op_name = match op_idx {
                            0 => "GetObject",
                            1 => "GetObjectRange",
                            2 => "PutObject",
                            3 => "PutObjectIfNoneMatch",
                            4 => "ListObjects",
                            5 => "HeadObject",
                            6 => "DeleteObject",
                            7 => "ObjectExists",
                            _ => "Unknown",
                        };
                        scan_stage.insert(
                            op_name.to_string(),
                            TrafficByOperation {
                                count: 0,
                                bytes,
                            },
                        );
                    }
                }
                traffic_by_stage.insert("scan".to_string(), scan_stage);

                // 3. Flush traffic counters to DB (for persistence)
                flush_counters(&counters, &db).await;

                // 4. Compute file type breakdown from diff_results
                let mut file_type_breakdown: HashMap<String, u64> = HashMap::new();

                for diff_result in &resp.diff_results {
                    for (ft, count) in &diff_result.file_type_counts {
                        *file_type_breakdown.entry(ft.clone()).or_insert(0) += count;
                    }
                }

                // 5. Compute size ranges from scan_objects
                let mut size_ranges = SizeRanges::default();

                for host in &resp.hosts {
                    let objects =
                        ScanObjectEntry::list_by_scan(&db, &resp.scan_id, &host.host_id).await?;
                    for obj in &objects {
                        match obj.size {
                            0..=1024 => size_ranges.tiny = size_ranges.tiny.saturating_add(1),
                            1025..=102400 => size_ranges.small = size_ranges.small.saturating_add(1),
                            102401..=1048576 => {
                                size_ranges.medium = size_ranges.medium.saturating_add(1)
                            }
                            1048577..=10485760 => {
                                size_ranges.large = size_ranges.large.saturating_add(1)
                            }
                            _ => size_ranges.huge = size_ranges.huge.saturating_add(1),
                        }
                    }
                }

                // 6. Compute scan status totals from diff results
                let mut total_files = 0u64;
                let mut new_files = 0u64;
                let mut changed_files = 0u64;
                let mut deleted_files = 0u64;

                for diff_result in &resp.diff_results {
                    new_files += diff_result.new_files;
                    changed_files += diff_result.changed_files;
                    deleted_files += diff_result.deleted_files;
                    total_files += diff_result.new_files
                        + diff_result.changed_files
                        + diff_result.unchanged_count;
                }

                // Total size from files table
                let total_size_val: Option<i64> = sqlx::query_scalar(
                    "SELECT SUM(size) FROM files \
                     WHERE host_id IN (SELECT host_id FROM scan_objects WHERE scan_id = ?) \
                     AND is_deleted = 0",
                )
                .bind(&resp.scan_id)
                .fetch_optional(&db)
                .await
                .map_err(|e| S3GalleryError::DbError(format!("Failed to sum sizes: {e}")))?;
                let total_size = total_size_val.unwrap_or(0) as u64;

                // 7. Calculate estimated cost ($0.09/GB download)
                let estimated_cost = total_download as f64 * 0.00000009;

                // 8. Clean up scan_objects
                ScanObjectEntry::delete_by_scan(&db, &resp.scan_id).await?;

                resp.report = Some(AggregateReport {
                    traffic_by_stage,
                    total_download_bytes: total_download,
                    total_upload_bytes: total_upload,
                    total_requests,
                    estimated_cost,
                    file_type_breakdown,
                    size_ranges,
                    total_files,
                    total_size,
                    new_files,
                    changed_files,
                    deleted_files,
                    host_count: resp.hosts.len() as u64,
                    duration_secs: start.elapsed().as_secs_f64(),
                });

                Ok(resp)
            }
        }))
    }
}