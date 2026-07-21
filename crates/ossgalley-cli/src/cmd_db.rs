use std::path::PathBuf;
use std::sync::Arc;
use ossgalley_core::error::{OssgalleyError, Result};
use ossgalley_core::types::*;
use ossgalley_core::s3::config::HostIdentifier;
use ossgalley_core::s3::client::S3Client;
use ossgalley_core::s3::mock::MockS3Client;
use ossgalley_core::s3::lock::{check_lock};
use ossgalley_core::db::status::{check_db_status, decide_action, DbStatus, DbAction};
use crate::cli::{Cli, DbCommands};

pub async fn run_db(cli: &Cli, host: &str, cmd: &DbCommands) -> Result<()> {
    let bucket = BucketName::new(&cli.bucket)
        .map_err(|e| OssgalleyError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
    let host_id = HostIdentifier::new(bucket.clone(), host)
        .map_err(|e| OssgalleyError::InvalidConfig(format!("Invalid host: {}", e)))?;

    let s3 = create_s3_client().await?;

    match cmd {
        DbCommands::Pull => {
            let db_path = PathBuf::from(".ossgallery").join("ossgallery.db");
            let status = check_db_status(&db_path, s3.as_ref(), &host_id).await?;
            let action = decide_action(&status, false);
            match action {
                DbAction::DownloadFromOss => {
                    let db_key = host_id.db_path();
                    let data = s3.get_object(&bucket, db_key).await?;
                    if let Some(parent) = db_path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(OssgalleyError::IoError)?;
                    }
                    std::fs::write(&db_path, &data)
                        .map_err(OssgalleyError::IoError)?;
                    println!("DB downloaded to: {}", db_path.display());
                }
                DbAction::UseLocal => {
                    println!("Local DB is up to date.");
                }
                DbAction::FullScanAndUpload => {
                    println!("No DB found. Run 'ossgalley scan {}' first.", host);
                }
                DbAction::Abort(msg) => {
                    eprintln!("{}", msg);
                }
            }
        }
        DbCommands::Push => {
            // Upload local DB to OSS
            let db_path = PathBuf::from(".ossgallery").join("ossgallery.db");
            let data = std::fs::read(&db_path)
                .map_err(OssgalleyError::IoError)?;
            let db_key = host_id.db_path();
            s3.put_object(&bucket, db_key, &data).await?;
            println!("DB uploaded to OSS.");
        }
        DbCommands::Status => {
            let db_path = PathBuf::from(".ossgallery").join("ossgallery.db");
            let status = check_db_status(&db_path, s3.as_ref(), &host_id).await?;
            match status {
                DbStatus::LocalCacheUpToDate => println!("Local DB is up to date."),
                DbStatus::RemoteNewer { .. } => println!("Remote DB is newer."),
                DbStatus::RemoteOnly => println!("DB exists only on remote."),
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
            println!("WARNING: This will force-unlock the DB. Other clients may have been scanning.");
            println!("Type 'yes' to confirm:");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)
                .map_err(|e| OssgalleyError::Internal(e.to_string()))?;
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

async fn create_s3_client() -> Result<Arc<dyn S3Client>> {
    Ok(Arc::new(MockS3Client::new()))
}