use std::collections::BTreeSet;
use std::sync::Arc;

use crate::cli::Cli;
use s3_gallery_core::db::models::HostConfigEntry;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::HostIdentifier;
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::scan::scanner::run_scan as core_run_scan;
use s3_gallery_core::scan::scanner::ScanConfig;
use s3_gallery_core::scan::scanner::ScanResult;
use s3_gallery_core::types::{BucketName, ObjectKey};
use sqlx::SqlitePool;

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

/// Create/connect to local DB, run migrations, return pool.
async fn setup_db_pool(cli: &Cli) -> Result<SqlitePool> {
    let db_path = &cli.db_path;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pool = create_pool(db_path).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

pub async fn run_init(cli: &Cli, prefix: Option<String>, opts: ScanOptions, confirm: bool) -> Result<()> {
    // Check if local DB already exists
    if cli.db_path.exists() && !confirm {
        return Err(S3GalleryError::Internal(
            format!("Local database exists at '{}'. Use --confirm to overwrite.", cli.db_path.display())
        ));
    }

    // If --confirm, remove existing DB file
    if cli.db_path.exists() && confirm {
        tracing::info!("--confirm set, removing existing database and re-scanning from scratch");
        std::fs::remove_file(&cli.db_path)?;
    }

    let (bucket, s3, bucket_str) = setup_scan_common(cli).await?;
    let pool = setup_db_pool(cli).await?;

    run_scan_core(&s3, &pool, &bucket, &bucket_str, prefix, &opts, cli).await
}

pub async fn run_update(cli: &Cli, prefix: Option<String>, opts: ScanOptions) -> Result<()> {
    // Check that local DB exists
    if !cli.db_path.exists() {
        return Err(S3GalleryError::NotFound(
            format!("No local database at '{}'. Run 's3-gallery scan init' first.", cli.db_path.display())
        ));
    }

    let (bucket, s3, bucket_str) = setup_scan_common(cli).await?;
    let pool = setup_db_pool(cli).await?;

    run_scan_core(&s3, &pool, &bucket, &bucket_str, prefix, &opts, cli).await
}

pub async fn run_sync(cli: &Cli, prefix: Option<String>, opts: ScanOptions) -> Result<()> {
    let (bucket, s3, bucket_str) = setup_scan_common(cli).await?;

    // Pull remote DB from OSS
    let db_key = ObjectKey::new("s3-gallery.db".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid db key: {e}")))?;

    match s3.get_object(&bucket, &db_key).await {
        Ok(data) => {
            tracing::info!("pulled remote DB ({} bytes)", data.len());
            if let Some(parent) = cli.db_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&cli.db_path, &data).map_err(S3GalleryError::IoError)?;
        }
        Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
            tracing::info!("no remote DB found, falling back to fresh scan");
            let pool = setup_db_pool(cli).await?;
            return run_scan_core(&s3, &pool, &bucket, &bucket_str, prefix, &opts, cli).await;
        }
        Err(e) => return Err(e),
    }

    let pool = setup_db_pool(cli).await?;
    run_scan_core(&s3, &pool, &bucket, &bucket_str, prefix, &opts, cli).await
}

/// Core scan logic: list OSS objects, discover hosts, update DB, push DB to remote.
async fn run_scan_core(
    s3: &Arc<dyn S3Client>,
    pool: &SqlitePool,
    bucket: &BucketName,
    bucket_str: &str,
    prefix: Option<String>,
    opts: &ScanOptions,
    cli: &Cli,
) -> Result<()> {
    let db_path = &cli.db_path;

    // Determine scan scope prefix
    let scope_prefix = prefix.unwrap_or_default();
    let scope_prefix_str = if scope_prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", scope_prefix.trim_end_matches('/'))
    };

    // Try to read host.config.json at the scope root
    let config_key_str = format!("{}.s3-gallery/host.config.json", scope_prefix_str);
    let config_key = ObjectKey::new(config_key_str.clone())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid config key: {e}")))?;

    let root_config = s3.get_object(bucket, &config_key).await.ok();

    if let Some(data) = root_config {
        // CASE 1: Root has config → scan entire scope as a single host
        let host: HostIdentifier = serde_json::from_slice(&data)
            .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

        tracing::info!(host_id = %host.host_id, name = %host.host_name, "found host config at scope root");

        let scan_prefix = ObjectKey::new(scope_prefix_str.clone())
            .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

        let result = scan_host(
            s3, pool, bucket, &host, &scan_prefix, opts,
        )
        .await?;

        // Save host config
        HostConfigEntry::upsert_host_config(
            pool, &host.host_id, bucket_str, &cli.endpoint, &cli.region,
        )
        .await?;

        println!("Scan complete:");
        println!("  Host: {} ({})", host.host_name, host.host_id);
        println!("  Total files: {}", result.total_files);
        println!("  New files: {}", result.new_files);
        println!("  Changed files: {}", result.changed_files);
        println!("  Deleted files: {}", result.deleted_files);
        println!("  Duration: {:.1}s", result.duration_secs);
    } else {
        // CASE 2: No root config → discover hosts in first-level subdirectories
        let list_prefix = ObjectKey::new(scope_prefix_str.clone())
            .map_err(|e| S3GalleryError::Internal(format!("Invalid prefix: {e}")))?;

        let all_objects = s3.list_objects(bucket, &list_prefix).await?;

        // Extract unique first-level directory names
        let mut subdirs: BTreeSet<String> = BTreeSet::new();
        for obj in &all_objects {
            let key = obj.key.as_str();
            if let Some(rest) = key.strip_prefix(&scope_prefix_str) {
                if let Some(slash) = rest.find('/') {
                    let dir = &rest[..slash];
                    if !dir.is_empty() {
                        subdirs.insert(dir.to_string());
                    }
                }
            }
        }

        let mut total_files: u64 = 0;
        let mut total_new: u64 = 0;
        let mut total_changed: u64 = 0;
        let mut total_deleted: u64 = 0;
        let mut host_count: u64 = 0;
        let mut duration_secs: f64 = 0.0;

        for dir in &subdirs {
            let dir_prefix_str = format!("{}{}/", scope_prefix_str, dir);
            let dir_config_key_str = format!("{}{}/.s3-gallery/host.config.json", scope_prefix_str, dir);
            let dir_config_key = ObjectKey::new(dir_config_key_str)
                .map_err(|e| S3GalleryError::Internal(format!("Invalid config key: {e}")))?;

            let dir_config = s3.get_object(bucket, &dir_config_key).await.ok();

            if let Some(data) = dir_config {
                // This subdirectory is a host
                let host: HostIdentifier = serde_json::from_slice(&data)
                    .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

                tracing::info!(host_id = %host.host_id, name = %host.host_name, dir = %dir, "found host config in subdirectory");

                let scan_prefix = ObjectKey::new(dir_prefix_str.clone())
                    .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                let result = scan_host(
                    s3, pool, bucket, &host, &scan_prefix, opts,
                )
                .await?;

                total_files += result.total_files;
                total_new += result.new_files;
                total_changed += result.changed_files;
                total_deleted += result.deleted_files;
                duration_secs += result.duration_secs;
                host_count += 1;

                // Save host config
                HostConfigEntry::upsert_host_config(
                    pool, &host.host_id, bucket_str, &cli.endpoint, &cli.region,
                )
                .await?;

                println!("  Host: {} ({}) — {} files", host.host_name, host.host_id, result.total_files);
            } else {
                // No host config → scan as regular directory, use dir name as host_id
                tracing::info!(dir = %dir, "no host config, scanning as regular directory");

                // Build a temporary host with dir name as host_id
                let temp_host = HostIdentifier::build(
                    dir.clone(),
                    dir,
                    "auto",
                    "",
                    bucket.clone(),
                    ObjectKey::new(dir_prefix_str.clone())
                        .map_err(|e| S3GalleryError::Internal(format!("invalid prefix: {e}")))?,
                    chrono::Utc::now().to_rfc3339(),
                    1,
                )?;

                let scan_prefix = ObjectKey::new(dir_prefix_str.clone())
                    .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                let result = scan_host(
                    s3, pool, bucket, &temp_host, &scan_prefix, opts,
                )
                .await?;

                total_files += result.total_files;
                total_new += result.new_files;
                total_changed += result.changed_files;
                total_deleted += result.deleted_files;
                duration_secs += result.duration_secs;

                // Save minimal host config (type=auto, no name) for serve to discover
                HostConfigEntry::upsert_host_config(
                    pool, dir, bucket_str, &cli.endpoint, &cli.region,
                )
                .await?;

                println!("  Directory: {} — {} files (no host config)", dir, result.total_files);
            }
        }

        println!("Scan complete:");
        if host_count > 0 {
            println!("  Hosts: {}", host_count);
        }
        println!("  Total files: {}", total_files);
        println!("  New files: {}", total_new);
        println!("  Changed files: {}", total_changed);
        println!("  Deleted files: {}", total_deleted);
        println!("  Duration: {:.1}s", duration_secs);
        if host_count == 0 {
            println!("# init: s3-gallery init --bucket {} --prefix <name> --name <display-name>", bucket_str);
        }
    }

    // Auto push DB to remote
    let db_key = ObjectKey::new("s3-gallery.db".to_string())
        .map_err(|e| S3GalleryError::Internal(format!("Invalid db key: {e}")))?;
    let db_data = std::fs::read(db_path).map_err(S3GalleryError::IoError)?;
    s3.put_object(bucket, &db_key, &db_data).await?;
    println!("  DB pushed to remote.");

    Ok(())
}

/// Scan a single host and return the scan result.
async fn scan_host(
    s3: &Arc<dyn S3Client>,
    pool: &SqlitePool,
    bucket: &BucketName,
    host: &HostIdentifier,
    scan_prefix: &ObjectKey,
    opts: &ScanOptions,
) -> Result<ScanResult> {
    let scan_config = ScanConfig {
        s3: s3.clone(),
        db: pool.clone(),
        bucket: bucket.clone(),
        prefix: scan_prefix.clone(),
        concurrency: opts.concurrency,
        extract_metadata: opts.extract_metadata,
        generate_thumbnails: opts.with_thumbnails,
        client_id: format!("cli-{}", host.host_id),
        host_id: host.host_id.clone(),
    };

    core_run_scan(scan_config).await
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