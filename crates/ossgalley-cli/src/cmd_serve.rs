use std::path::PathBuf;
use std::sync::Arc;
use ossgalley_core::error::{OssgalleyError, Result};
use ossgalley_core::types::*;
use ossgalley_core::s3::config::HostIdentifier;
use ossgalley_core::s3::mock::MockS3Client;
use ossgalley_core::s3::client::S3Client;
use ossgalley_core::db::pool::create_pool;
use ossgalley_core::db::schema::run_migrations;
use ossgalley_core::db::status::{check_db_status, decide_action, DbAction};
use ossgalley_core::view::LocalView;
use ossgalley_core::view::remote::RemoteView;
use crate::cli::Cli;

pub async fn run_serve(cli: &Cli, host: &str, port: u16, readonly: bool) -> Result<()> {
    let bucket = BucketName::new(&cli.bucket)
        .map_err(|e| OssgalleyError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
    let host_id = HostIdentifier::new(bucket.clone(), host)
        .map_err(|e| OssgalleyError::InvalidConfig(format!("Invalid host: {}", e)))?;

    // Ensure DB is available
    let db_path = PathBuf::from(".ossgallery").join("ossgallery.db");
    let s3 = Arc::new(MockS3Client::new()) as Arc<dyn S3Client>;

    // Check DB status
    let status = check_db_status(&db_path, s3.as_ref(), &host_id).await?;
    let action = decide_action(&status, readonly);

    match action {
        DbAction::UseLocal | DbAction::DownloadFromOss => {
            // Proceed with existing DB
            if !db_path.exists() {
                let db_key = host_id.db_path();
                let data = s3.get_object(&bucket, db_key).await?;
                if let Some(parent) = db_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(OssgalleyError::IoError)?;
                }
                std::fs::write(&db_path, &data)
                    .map_err(OssgalleyError::IoError)?;
                println!("DB downloaded from OSS.");
            }
        }
        DbAction::FullScanAndUpload => {
            return Err(OssgalleyError::NotFound(
                "No database found. Run 'ossgalley scan <host>' first.".to_string()
            ));
        }
        DbAction::Abort(msg) => {
            return Err(OssgalleyError::Internal(msg));
        }
    }

    // Create pool, run migrations, set up views
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;
    let _local_view = LocalView::new(pool.clone());
    let _remote_view = RemoteView::new(pool, s3, bucket);

    // TODO: Start web server
    // For now, just print that it would start
    println!("ossgalley web server starting on port {}...", port);
    println!("  Host: {}", host);
    println!("  Read-only: {}", readonly);
    println!("  Press Ctrl+C to stop.");

    // Block forever
    let () = std::future::pending().await;

    Ok(())
}