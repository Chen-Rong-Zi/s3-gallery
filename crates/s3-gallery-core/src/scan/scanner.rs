use sqlx::SqlitePool;
use std::sync::Arc;
use tower::Service;
use tower::ServiceBuilder;

use crate::error::Result;
use crate::s3::layers::{LogLayer, TrafficLayer};
use crate::s3::s3_service::S3Service;
use crate::s3::traffic_persist::spawn_batch_writer;
use crate::s3::traffic_recorder::TrafficRecorder;
use crate::scan::aggregate::AggregateLayer;
use crate::scan::diff_layer::DiffLayer;
use crate::scan::discover::DiscoverLayer;
use crate::scan::pipeline::ScanRequest;
use crate::scan::process::ProcessLayer;
use crate::types::{BucketName, ObjectKey};

/// Configuration for a scan operation.
pub struct ScanConfig {
    pub s3: S3Service,
    pub db: SqlitePool,
    pub bucket: BucketName,
    pub prefix: ObjectKey,
    pub concurrency: usize,
    pub extract_metadata: bool,
    pub generate_thumbnails: bool,
    pub client_id: String,
    pub host_id: String,
}

/// Result of a scan operation.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub total_files: u64,
    pub total_size: u64,
    pub new_files: u64,
    pub changed_files: u64,
    pub deleted_files: u64,
    pub metadata_extracted: u64,
    pub duration_secs: f64,
}

/// Run a full scan using the pipeline internally.
///
/// This is a backward-compatible wrapper around the pipeline.
///
/// # Errors
///
/// Returns an error if any S3 or database operation fails.
pub async fn run_scan(config: ScanConfig, endpoint: String) -> Result<ScanResult> {
    tracing::info!(
        target: "s3_gallery::scan",
        prefix = %config.prefix,
        client_id = %config.client_id,
        "Scan started"
    );

    let tx = spawn_batch_writer(config.db.clone(), 60, 100);
    let recorder = Arc::new(TrafficRecorder::new(tx));

    let discover_s3 = ServiceBuilder::new()
        .layer(LogLayer)
        .layer(TrafficLayer::new(
            recorder.clone(),
            &config.host_id,
            "scan_discover",
        ))
        .service(S3Service::new(config.s3.clone().into_inner()));

    let exif_s3 = ServiceBuilder::new()
        .layer(LogLayer)
        .layer(TrafficLayer::new(
            recorder.clone(),
            &config.host_id,
            "scan_exif",
        ))
        .service(S3Service::new(config.s3.into_inner()));

    let mut pipeline = ServiceBuilder::new()
        .layer(AggregateLayer::new(config.db.clone()))
        .layer(ProcessLayer::new(config.db.clone(), exif_s3, config.concurrency))
        .layer(DiffLayer::new(config.db.clone()))
        .layer(DiscoverLayer::new(config.db.clone()))
        .service(discover_s3);

    let resp = pipeline
        .call(ScanRequest {
            endpoint,
            bucket: config.bucket.clone(),
            scope_prefix: config.prefix.clone(),
            concurrency: config.concurrency,
            extract_metadata: config.extract_metadata,
            generate_thumbnails: config.generate_thumbnails,
            client_id: config.client_id.clone(),
        })
        .await?;

    // Convert AggregateReport to ScanResult
    if let Some(report) = &resp.report {
        Ok(ScanResult {
            total_files: report.total_files,
            total_size: report.total_size,
            new_files: report.new_files,
            changed_files: report.changed_files,
            deleted_files: report.deleted_files,
            metadata_extracted: 0,
            duration_secs: report.duration_secs,
        })
    } else {
        Ok(ScanResult {
            total_files: 0,
            total_size: 0,
            new_files: 0,
            changed_files: 0,
            deleted_files: 0,
            metadata_extracted: 0,
            duration_secs: 0.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::s3::client::S3Client;
    use crate::s3::mock::MockS3Client;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_scan_empty_bucket() -> Result<()> {
        let dir = tempdir().map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await?;
        run_migrations(&pool).await?;

        let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;

        let config = ScanConfig {
            s3: S3Service::new(s3.clone()),
            db: pool.clone(),
            bucket: BucketName::new("test-bucket")?,
            prefix: ObjectKey::new("test")?,
            concurrency: 10,
            extract_metadata: false,
            generate_thumbnails: false,
            client_id: "test-client".to_string(),
            host_id: "test-host".to_string(),
        };

        let result = run_scan(config, String::new()).await?;
        assert_eq!(result.total_files, 0);
        assert_eq!(result.new_files, 0);
        Ok(())
    }

    // Note: More comprehensive tests would require adding objects to the MockS3Client,
    // which currently only implements the S3Client trait but may not have a way to
    // add test objects. In a real test suite, we'd extend MockS3Client for that.
}