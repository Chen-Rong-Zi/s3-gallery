use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;
use crate::cli::Cli;
use ossgalley_core::error::{OssgalleyError, Result};
use ossgalley_core::s3::config::HostIdentifier;
use ossgalley_core::types::BucketName;
use ossgalley_core::s3::lock::acquire_lock;
use ossgalley_core::s3::client::S3Client;
use ossgalley_core::db::pool::create_pool;
use ossgalley_core::db::schema::run_migrations;
use ossgalley_core::scan::scanner::ScanConfig;
use ossgalley_core::scan::scanner::run_scan as core_run_scan;

pub async fn run_scan(cli: &Cli, host: &str, opts: ScanOptions) -> Result<()> {
    let bucket = BucketName::new(&cli.bucket)
        .map_err(|e| OssgalleyError::InvalidConfig(format!("Invalid bucket: {}", e)))?;

    let host_id = HostIdentifier::new(bucket.clone(), host)
        .map_err(|e| OssgalleyError::InvalidConfig(format!("Invalid host: {}", e)))?;

    // Create local DB directory and pool
    let db_path = PathBuf::from(".ossgallery").join("ossgallery.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;

    // Create S3 client
    let s3_client = create_s3_client(cli).await?;

    // Acquire distributed lock
    let lock_key = host_id.lock_path();
    let client_id = Uuid::new_v4().to_string();
    let _lock = acquire_lock(s3_client.clone(), bucket.clone(), lock_key.clone(), client_id.clone()).await?;

    // Run scan
    let config = ScanConfig {
        s3: s3_client.clone(),
        db: pool,
        bucket: bucket.clone(),
        prefix: host_id.prefix.clone(),
        concurrency: opts.concurrency,
        extract_metadata: opts.extract_metadata,
        generate_thumbnails: opts.with_thumbnails,
        client_id,
    };

    let result = core_run_scan(config).await?;

    println!("Scan complete:");
    println!("  Total files: {}", result.total_files);
    println!("  New files: {}", result.new_files);
    println!("  Changed files: {}", result.changed_files);
    println!("  Deleted files: {}", result.deleted_files);
    println!("  Duration: {:.1}s", result.duration_secs);

    Ok(())
}

/// Create an S3 client from CLI config
async fn create_s3_client(_cli: &Cli) -> Result<Arc<dyn S3Client>> {
    // For now, create a mock client
    // TODO: Replace with real AWS SDK S3 client
    use ossgalley_core::s3::mock::MockS3Client;
    Ok(Arc::new(MockS3Client::new()))
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