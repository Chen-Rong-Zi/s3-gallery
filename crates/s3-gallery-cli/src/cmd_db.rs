use crate::cli::{Cli, DbCommands};
use s3_gallery_core::db::status::{check_db_status, DbStatus};
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::client::S3Client;
use s3_gallery_core::s3::config::HostIdentifier;
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::lock::check_lock;
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::types::*;
use std::sync::Arc;

pub async fn run_db(cli: &Cli, host: &str, cmd: &DbCommands) -> Result<()> {
    let bucket_str = cli.bucket.as_ref().ok_or_else(|| {
        S3GalleryError::InvalidConfig(
            "--bucket is required for DB commands. Use: s3-gallery db --bucket <name> <host> <command>".to_string(),
        )
    })?;
    let bucket = BucketName::new(bucket_str)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
    let host_id = HostIdentifier::new(bucket.clone(), host)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid host: {}", e)))?;

    let s3 = create_s3_client(cli).await?;

    match cmd {
        DbCommands::Pull => {
            let db_path = &cli.db_path;
            let db_key = host_id.db_path();
            match s3.get_object(&bucket, db_key).await {
                Ok(data) => {
                    if let Some(parent) = db_path.parent() {
                        std::fs::create_dir_all(parent).map_err(S3GalleryError::IoError)?;
                    }
                    std::fs::write(db_path, &data).map_err(S3GalleryError::IoError)?;
                    println!("DB downloaded to: {}", db_path.display());
                }
                Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
                    println!(
                        "No remote DB found at '{}'. Run 's3-gallery scan {}' first.",
                        db_key.as_str(),
                        host
                    );
                }
                Err(e) => return Err(e),
            }
        }
        DbCommands::Push => {
            // Upload local DB to OSS
            let db_path = &cli.db_path;
            let data = std::fs::read(db_path).map_err(S3GalleryError::IoError)?;
            let db_key = host_id.db_path();
            s3.put_object(&bucket, db_key, &data).await?;
            println!("DB uploaded to OSS.");
        }
        DbCommands::Status => {
            let db_path = &cli.db_path;
            let status = check_db_status(db_path).await?;
            match status {
                DbStatus::LocalExists => println!("Local DB exists."),
                DbStatus::None => println!("No DB found."),
            }
        }
        DbCommands::Lock => {
            let lock_key = host_id.lock_path();
            let locked = check_lock(s3.as_ref(), &bucket, lock_key).await?;
            if locked {
                println!("Lock is held.");
            } else {
                println!("Lock is free.");
            }
        }
        DbCommands::Unlock => {
            println!(
                "WARNING: This will force-unlock the DB. Other clients may have been scanning."
            );
            println!("Type 'yes' to confirm:");
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            if input.trim() == "yes" {
                s3.delete_object(&bucket, host_id.lock_path()).await?;
                println!("Lock released.");
            } else {
                println!("Unlock cancelled.");
            }
        }
    }

    Ok(())
}

async fn create_s3_client(cli: &Cli) -> Result<Arc<dyn S3Client>> {
    let bucket_str = cli.bucket.as_ref().ok_or_else(|| {
        S3GalleryError::InvalidConfig("--bucket is required for DB commands".to_string())
    })?;
    let bucket = BucketName::new(bucket_str)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
    let config = OssConfig::validate(
        bucket,
        &cli.endpoint,
        &cli.region,
        &cli.access_key,
        &cli.secret_key,
        10,
    )?;
    let client = RealS3Client::from_config(&config);
    Ok(Arc::new(client))
}
