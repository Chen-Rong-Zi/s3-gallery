use std::path::PathBuf;
use crate::cli::Cli;
use chrono::Utc;
use ossgalley_core::error::{OssgalleyError, Result};
use ossgalley_core::types::*;
use ossgalley_core::s3::config::HostIdentifier;
use ossgalley_core::db::pool::create_pool;
use ossgalley_core::db::schema::run_migrations;
use ossgalley_core::db::models::HostConfigEntry;

pub async fn run_init(cli: &Cli, host: &str) -> Result<()> {
    // Validate bucket
    let bucket = BucketName::new(&cli.bucket)
        .map_err(|e| OssgalleyError::InvalidConfig(format!("Invalid bucket: {}", e)))?;

    // Validate host
    let host_id = HostIdentifier::new(bucket.clone(), host)
        .map_err(|e| OssgalleyError::InvalidConfig(format!("Invalid host: {}", e)))?;

    // Create local DB
    let db_path = PathBuf::from(".ossgallery").join("ossgallery.db");
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;

    // Insert host config
    let config_entry = HostConfigEntry {
        host_id: host_id.host_id.clone(),
        host_name: host.to_string(),
        host_type: "unknown".to_string(),
        description: String::new(),
        created_at: Utc::now().to_rfc3339(),
    };
    HostConfigEntry::insert(&pool, &config_entry).await?;

    println!("Initialized host '{}' in bucket '{}'", host, cli.bucket);
    println!("  Local DB: {}", db_path.display());
    println!("  OSS path: {}", host_id.prefix.as_str());

    Ok(())
}