//! AggregateLayer — reads DB, produces AggregateReport.
//!
//! This is the outermost pipeline layer, generic over inner service.
//! Traffic data is queried from traffic_log since the batch writer persists
//! records in near-real-time.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use sea_orm::{DatabaseConnection, Statement};
use tokio::sync::Mutex;
use tower::service_fn;
use tower::util::BoxService;
use tower::{Layer, Service};

use crate::error::S3GalleryError;
use crate::s3::traffic_persist::BatchWriterHandle;
use crate::scan::pipeline::{
    AggregateReport, ScanRequest, ScanResponse, SizeRanges, TrafficByOperation,
};
use crate::scan::scan_objects::ScanObjectEntry;

/// AggregateLayer wraps an inner service with report generation.
pub struct AggregateLayer {
    db: DatabaseConnection,
    batch_writer: Arc<tokio::sync::Mutex<Option<BatchWriterHandle>>>,
}

impl AggregateLayer {
    /// Create a new `AggregateLayer`.
    ///
    /// Pass the `BatchWriterHandle` from `spawn_batch_writer()` to enable
    /// flushing buffered traffic records before generating the report.
    pub fn new(db: DatabaseConnection, batch_writer: Option<BatchWriterHandle>) -> Self {
        Self {
            db,
            batch_writer: Arc::new(tokio::sync::Mutex::new(batch_writer)),
        }
    }
}

impl<I> Layer<I> for AggregateLayer
where
    I: Service<ScanRequest, Response = ScanResponse, Error = S3GalleryError> + Send + 'static,
    I::Future: Send,
{
    type Service = BoxService<ScanRequest, ScanResponse, S3GalleryError>;

    fn layer(&self, inner: I) -> Self::Service {
        let db = self.db.clone();
        let batch_writer = self.batch_writer.clone();
        let inner = Arc::new(Mutex::new(inner));
        BoxService::new(service_fn(move |req: ScanRequest| {
            let db = db.clone();
            let batch_writer = batch_writer.clone();
            let inner = inner.clone();
            let start = Instant::now();
            let scan_start = Utc::now();
            async move {
                // 1. Call inner chain
                let mut resp = {
                    let mut inner = inner.lock().await;
                    inner.call(req).await?
                };

                // 2. Flush batch writer to ensure all traffic is in DB
                if let Some(handle) = batch_writer.lock().await.as_ref() {
                    handle.flush().await;
                }

                let scan_end = Utc::now();
                let scan_start_str = scan_start.to_rfc3339();
                let scan_end_str = scan_end.to_rfc3339();

                // 3. Query traffic from DB for this scan period
                let rows: Vec<(String, String, String, i64, i64)> = {
                    use sea_orm::ConnectionTrait;
                    let stmt = Statement::from_sql_and_values(
                        sea_orm::DatabaseBackend::Sqlite,
                        "SELECT business, operation, direction, \
                         COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
                         FROM traffic_log \
                         WHERE recorded_at >= ? AND recorded_at <= ? \
                           AND business LIKE 'scan_%' \
                         GROUP BY business, operation, direction",
                        vec![
                            sea_orm::Value::String(Some(Box::new(scan_start_str.clone()))),
                            sea_orm::Value::String(Some(Box::new(scan_end_str.clone()))),
                        ],
                    );
                    db.query_all(stmt)
                        .await
                        .map_err(|e| {
                            S3GalleryError::DbError(format!("Failed to query traffic: {e}"))
                        })?
                        .into_iter()
                        .map(|row| {
                            let business: String = row
                                .try_get_by("business")
                                .or_else(|_| row.try_get_by(0))
                                .unwrap_or_default();
                            let operation: String = row
                                .try_get_by("operation")
                                .or_else(|_| row.try_get_by(1))
                                .unwrap_or_default();
                            let direction: String = row
                                .try_get_by("direction")
                                .or_else(|_| row.try_get_by(2))
                                .unwrap_or_default();
                            let bytes: i64 = row
                                .try_get_by("COALESCE(SUM(bytes), 0)")
                                .or_else(|_| row.try_get_by(3))
                                .unwrap_or(0);
                            let count: i64 = row
                                .try_get_by("COALESCE(SUM(count), 0)")
                                .or_else(|_| row.try_get_by(4))
                                .unwrap_or(0);
                            (business, operation, direction, bytes, count)
                        })
                        .collect::<Vec<_>>()
                };

                let mut traffic_by_stage: HashMap<String, HashMap<String, TrafficByOperation>> =
                    HashMap::new();
                let mut total_download: u64 = 0;
                let mut total_upload: u64 = 0;
                let mut total_requests: u64 = 0;

                for (business, operation, direction, bytes, count) in &rows {
                    let bytes = *bytes as u64;
                    let count = *count as u64;

                    let stage = traffic_by_stage.entry(business.clone()).or_default();
                    stage.insert(operation.clone(), TrafficByOperation { count, bytes });

                    if direction == "download" {
                        total_download = total_download.saturating_add(bytes);
                    } else {
                        total_upload = total_upload.saturating_add(bytes);
                    }
                    total_requests = total_requests.saturating_add(count);
                }

                // 3. Compute file type breakdown from diff_results
                let mut file_type_breakdown: HashMap<String, u64> = HashMap::new();

                for diff_result in &resp.diff_results {
                    for (ft, count) in &diff_result.file_type_counts {
                        *file_type_breakdown.entry(ft.clone()).or_insert(0) += count;
                    }
                }

                // 4. Compute size ranges from scan_objects
                let mut size_ranges = SizeRanges::default();

                for host in &resp.hosts {
                    let objects =
                        ScanObjectEntry::list_by_scan(&db, &resp.scan_id, &host.host_id).await?;
                    for obj in &objects {
                        match obj.size {
                            0..=1024 => size_ranges.tiny = size_ranges.tiny.saturating_add(1),
                            1025..=102400 => {
                                size_ranges.small = size_ranges.small.saturating_add(1)
                            }
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

                // 5. Compute scan status totals from diff results
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
                let total_size: u64 = {
                    use sea_orm::ConnectionTrait;
                    let stmt = Statement::from_sql_and_values(
                        sea_orm::DatabaseBackend::Sqlite,
                        "SELECT COALESCE(SUM(size), 0) FROM files \
                         WHERE host_id IN (SELECT host_id FROM scan_objects WHERE scan_id = ?) \
                         AND is_deleted = 0",
                        vec![sea_orm::Value::String(Some(Box::new(resp.scan_id.clone())))],
                    );
                    db.query_one(stmt)
                        .await
                        .map_err(|e| S3GalleryError::DbError(format!("Failed to sum sizes: {e}")))?
                        .and_then(|row| {
                            row.try_get_by::<i64, usize>(0)
                                .or_else(|_| row.try_get_by::<i64, &str>("COALESCE(SUM(size), 0)"))
                                .ok()
                        })
                        .unwrap_or(0) as u64
                };

                // 6. Calculate estimated cost ($0.09/GB download)
                let estimated_cost = total_download as f64 * 0.00000009;

                // 7. Clean up scan_objects
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate::run_full_migration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_aggregate_layer_creation() -> crate::error::Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = sqlx::SqlitePool::connect(&db_url)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db = sea_orm::Database::connect(&db_url)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        run_full_migration(&db).await?;

        let layer = AggregateLayer::new(db, None);
        // Just verify it constructs without error
        assert!(true);

        Ok(())
    }
}
