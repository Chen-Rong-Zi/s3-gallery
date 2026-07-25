use std::sync::Arc;

use sea_orm::DatabaseConnection;
use sqlx::SqlitePool;
use tower::Service;
use tower::ServiceBuilder;

use crate::cli::Cli;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::layers::{LogLayer, TrafficLayer};
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::traffic_persist::spawn_batch_writer;
use s3_gallery_core::s3::traffic_recorder::TrafficRecorder;
use s3_gallery_core::scan::aggregate::AggregateLayer;
use s3_gallery_core::scan::diff_layer::DiffLayer;
use s3_gallery_core::scan::discover::DiscoverLayer;
use s3_gallery_core::scan::pipeline::ScanRequest;
use s3_gallery_core::scan::process::ProcessLayer;
use s3_gallery_core::types::{BucketName, ObjectKey};

/// Parse bucket from CLI, create S3 client, and return bucket info.
async fn setup_scan_common(cli: &Cli) -> Result<(BucketName, Arc<dyn S3Client>, String)> {
    let bucket_str = cli.bucket.as_ref().ok_or_else(|| {
        S3GalleryError::InvalidConfig(
            "--bucket is required for scan. Use: s3-gallery scan <init|update|sync> --bucket <name>".to_string(),
        )
    })?;
    let bucket = BucketName::new(bucket_str)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?;

    // Create S3 client
    let config = OssConfig::validate(
        bucket.clone(),
        &cli.endpoint,
        &cli.region,
        &cli.access_key,
        &cli.secret_key,
        10,
    )?;
    let s3 = Arc::new(RealS3Client::from_config(&config)) as Arc<dyn S3Client>;

    Ok((bucket, s3, bucket_str.to_string()))
}

/// Create/connect to local DB, run migrations, return connection.
async fn setup_db_pool(cli: &Cli) -> Result<DatabaseConnection> {
    let db_path = &cli.db_path;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = create_pool(db_path).await?;
    run_migrations(db.get_sqlite_connection_pool()).await?;
    Ok(db)
}

pub async fn run_init(
    cli: &Cli,
    prefix: Option<String>,
    opts: ScanOptions,
    confirm: bool,
) -> Result<()> {
    // Check if local DB already exists
    if cli.db_path.exists() && !confirm {
        return Err(S3GalleryError::Internal(format!(
            "Local database exists at '{}'. Use --confirm to overwrite.",
            cli.db_path.display()
        )));
    }

    // If --confirm, remove existing DB file
    if cli.db_path.exists() && confirm {
        tracing::info!("--confirm set, removing existing database and re-scanning from scratch");
        std::fs::remove_file(&cli.db_path)?;
    }

    let (bucket, s3, _bucket_str) = setup_scan_common(cli).await?;
    let db = setup_db_pool(cli).await?;

    run_scan_core(s3.clone(), &db, &bucket, &prefix.unwrap_or_default(), &opts, cli).await
}

pub async fn run_update(cli: &Cli, prefix: Option<String>, opts: ScanOptions) -> Result<()> {
    // Check that local DB exists
    if !cli.db_path.exists() {
        return Err(S3GalleryError::NotFound(format!(
            "No local database at '{}'. Run 's3-gallery scan init' first.",
            cli.db_path.display()
        )));
    }

    let (bucket, s3, _bucket_str) = setup_scan_common(cli).await?;
    let db = setup_db_pool(cli).await?;

    run_scan_core(s3.clone(), &db, &bucket, &prefix.unwrap_or_default(), &opts, cli).await
}

pub async fn run_sync(cli: &Cli, prefix: Option<String>, opts: ScanOptions) -> Result<()> {
    let (bucket, s3, _bucket_str) = setup_scan_common(cli).await?;
    let scope_prefix = prefix.unwrap_or_default();

    // Pull remote DB from OSS
    let db_key = ObjectKey::new("s3-gallery.db".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid db key: {e}")))?;

    let mut s3_svc = S3Service::new(s3.clone());
    match s3_svc.get_object(&bucket, &db_key).await {
        Ok(data) => {
            tracing::info!("pulled remote DB ({} bytes)", data.len());
            if let Some(parent) = cli.db_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&cli.db_path, &data).map_err(S3GalleryError::IoError)?;
        }
        Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
            tracing::info!("no remote DB found, falling back to fresh scan");
            let db = setup_db_pool(cli).await?;
            return run_scan_core(s3.clone(), &db, &bucket, &scope_prefix, &opts, cli).await;
        }
        Err(e) => return Err(e),
    }

    let db = setup_db_pool(cli).await?;
    run_scan_core(s3.clone(), &db, &bucket, &scope_prefix, &opts, cli).await
}

/// Core scan logic: use the pipeline to discover hosts, diff, process, and generate report.
async fn run_scan_core(
    s3: Arc<dyn S3Client>,
    db: &DatabaseConnection,
    bucket: &BucketName,
    scope_prefix: &str,
    opts: &ScanOptions,
    cli: &Cli,
) -> Result<()> {
    let db_path = &cli.db_path;

    // Set up traffic tracking with channel-based batch writer
    // spawn_batch_writer needs a raw SqlitePool, so create one separately
    let sqlite_pool = SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.display()))
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to create pool for batch writer: {e}")))?;
    let handle = spawn_batch_writer(sqlite_pool, 5, 100);
    let recorder = Arc::new(TrafficRecorder::new(handle.sender.clone()));

    // Build discover_s3 with LogLayer + TrafficLayer for "scan_discover"
    let discover_core = S3Service::new(s3.clone());
    let discover_s3 = ServiceBuilder::new()
        .layer(LogLayer)
        .layer(TrafficLayer::new(recorder.clone(), "discover", "scan_discover"))
        .service(discover_core);

    // Build exif_s3 with LogLayer + TrafficLayer for "scan_exif"
    let exif_core = S3Service::new(s3.clone());
    let exif_s3 = ServiceBuilder::new()
        .layer(LogLayer)
        .layer(TrafficLayer::new(recorder.clone(), "exif", "scan_exif"))
        .service(exif_core);

    // Build the pipeline: AggregateLayer -> ProcessLayer -> DiffLayer -> DiscoverLayer -> discover_s3
    // ServiceBuilder applies layers from outside-in, so the LAST layer is the outermost wrapper.
    // We want: AggregateLayer(ProcessLayer(DiffLayer(DiscoverLayer(discover_s3))))
    // So: ServiceBuilder::new().layer(Aggregate).layer(Process).layer(Diff).layer(Discover).service(discover_s3)
    let scope_prefix_key = ObjectKey::new(scope_prefix.to_string())
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

    let sqlite_pool = db.get_sqlite_connection_pool();

    let mut pipeline = ServiceBuilder::new()
        .layer(AggregateLayer::new(db.clone(), Some(handle)))
        .layer(ProcessLayer::new(sqlite_pool.clone(), exif_s3, opts.concurrency))
        .layer(DiffLayer::new(sqlite_pool.clone(), db.clone()))
        .layer(DiscoverLayer::new(sqlite_pool.clone(), db.clone()))
        .service(discover_s3);

    let resp = pipeline
        .call(ScanRequest {
            endpoint: cli.endpoint.clone(),
            bucket: bucket.clone(),
            scope_prefix: scope_prefix_key,
            concurrency: opts.concurrency,
            extract_metadata: opts.extract_metadata,
            generate_thumbnails: opts.with_thumbnails,
            client_id: format!("cli-{}", bucket.as_str()),
        })
        .await?;

    // Print report
    if let Some(report) = &resp.report {
        println!("Scan complete:");
        println!("  Hosts: {}", report.host_count);
        println!("  Total files: {}", report.total_files);
        println!("  Total size: {} bytes", report.total_size);
        println!("  New files: {}", report.new_files);
        println!("  Changed files: {}", report.changed_files);
        println!("  Deleted files: {}", report.deleted_files);
        println!("  Duration: {:.1}s", report.duration_secs);
        println!(
            "  Traffic (download): {:.2} MB",
            report.total_download_bytes as f64 / 1_000_000.0
        );
        println!(
            "  Traffic (upload): {:.2} MB",
            report.total_upload_bytes as f64 / 1_000_000.0
        );
        println!("  Total requests: {}", report.total_requests);
        println!("  Estimated cost: ${:.4}", report.estimated_cost);

        // File type breakdown
        if !report.file_type_breakdown.is_empty() {
            println!("  File types:");
            for (ft, count) in &report.file_type_breakdown {
                println!("    {}: {}", ft, count);
            }
        }
    }

    // Auto push DB to remote
    let db_key = ObjectKey::new("s3-gallery.db".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid db key: {e}")))?;
    let db_data = std::fs::read(db_path).map_err(S3GalleryError::IoError)?;
    let mut s3_svc = S3Service::new(s3.clone());
    s3_svc.put_object(bucket, &db_key, &db_data).await?;
    println!("  DB pushed to remote.");

    Ok(())
}

#[derive(Debug, clap::Parser)]
pub struct ScanOptions {
    pub incremental: bool,
    pub schedule: bool,
    pub force: bool,
    pub extract_metadata: bool,
    #[clap(long = "with-thumbnails")]
    pub with_thumbnails: bool,
    pub concurrency: usize,
}
