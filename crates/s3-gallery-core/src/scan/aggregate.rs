//! AggregateLayer — reads DB + TrafficCounters, produces AggregateReport.
//!
//! This is the outermost pipeline layer, generic over inner service.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use sqlx::SqlitePool;
use tower::{Layer, Service};

use crate::error::{Result, S3GalleryError};
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
    I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError> + Clone,
{
    type Service = AggregateService<I>;

    fn layer(&self, inner: I) -> Self::Service {
        AggregateService {
            inner,
            db: self.db.clone(),
            counters: self.counters.clone(),
        }
    }
}

/// AggregateService — calls inner, then generates AggregateReport from DB + counters.
pub struct AggregateService<I> {
    inner: I,
    db: SqlitePool,
    counters: Arc<TrafficCounters>,
}

impl<I> Service<ScanRequest> for AggregateService<I>
where
    I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError> + Clone + Send + 'static,
    I::Future: Send,
{
    type Response = ScanResponse;
    type Error = S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ScanRequest) -> Self::Future {
        let mut inner = self.inner.clone();
        let db = self.db.clone();
        let counters = self.counters.clone();
        let start = Instant::now();

        Box::pin(async move {
            // 1. Call inner chain
            let mut resp = inner.call(req).await?;

            // 2. Flush traffic counters to DB
            flush_counters(&counters, &db).await;

            // 3. Read traffic from DB for scan_discover and scan_exif
            let mut traffic_by_stage: HashMap<String, HashMap<String, TrafficByOperation>> =
                HashMap::new();

            let traffic_rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
                "SELECT business, operation, SUM(bytes), SUM(count) \
                 FROM traffic_log \
                 WHERE business IN ('scan_discover', 'scan_exif') \
                 AND recorded_at >= datetime('now', '-1 hour') \
                 GROUP BY business, operation",
            )
            .fetch_all(&db)
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to read traffic: {e}")))?;

            let mut total_download = 0u64;
            let mut total_upload = 0u64;
            let mut total_requests = 0u64;

            for (business, operation, bytes, count) in &traffic_rows {
                let stage = traffic_by_stage.entry(business.clone()).or_default();
                stage.insert(
                    operation.clone(),
                    TrafficByOperation {
                        count: *count as u64,
                        bytes: *bytes as u64,
                    },
                );

                match operation.as_str() {
                    "GetObject" | "GetObjectRange" | "HeadObject" | "ListObjects" => {
                        total_download = total_download.saturating_add(*bytes as u64);
                    }
                    "PutObject" | "PutObjectIfNoneMatch" => {
                        total_upload = total_upload.saturating_add(*bytes as u64);
                    }
                    _ => {
                        total_download = total_download.saturating_add(*bytes as u64);
                    }
                }
                total_requests = total_requests.saturating_add(*count as u64);
            }

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
        })
    }
}