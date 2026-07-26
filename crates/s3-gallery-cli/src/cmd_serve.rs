use std::net::SocketAddr;
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower::layer::Layer;

use s3_gallery_core::db::migrate::run_full_migration;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::status::{check_db_status, decide_action, DbAction};
use s3_gallery_core::entity::host_config;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::layers::LogLayer;
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::traffic_persist::spawn_batch_writer;
use s3_gallery_core::s3::traffic_recorder::TrafficRecorder;
use s3_gallery_core::types::BucketName;
use s3_gallery_core::types::HostId;

use crate::cli::Cli;
use crate::web::router::create_router;
use crate::web::state::AppState;

#[derive(Debug, Clone, sqlx::FromRow)]
struct HostConfigEntry {
    pub host_id: String,
    pub host_name: String,
    pub host_type: String,
    pub description: String,
    pub created_at: String,
    pub bucket: String,
    pub endpoint: String,
    pub region: String,
}

pub async fn run_serve(cli: &Cli, port: u16, readonly: bool, prefix: Option<String>) -> Result<()> {
    let db_path = &cli.db_path;

    // Check DB status
    let status = check_db_status(db_path).await?;
    let action = decide_action(&status, readonly);

    match action {
        DbAction::UseLocal => {
            // Local DB exists, use it
        }
        DbAction::FullScan => {
            return Err(S3GalleryError::NotFound(
                "No database found. Run 's3-gallery scan <host>' first.".to_string(),
            ));
        }
        DbAction::Abort(msg) => {
            return Err(S3GalleryError::Internal(msg));
        }
    }

    // Create pool, run migrations
    let db = create_pool(db_path).await?;
    run_full_migration(&db).await?;

    // Read all hosts from DB, or discover from files table
    let mut hosts: Vec<HostConfigEntry> =
        sqlx::query_as("SELECT * FROM host_config ORDER BY host_id")
            .fetch_all(db.get_sqlite_connection_pool())
            .await
            .map_err(|e| S3GalleryError::DbError(format!("Failed to list hosts: {e}")))?;
    if hosts.is_empty() {
        // No host_config entries — probe files table for distinct host_ids
        tracing::info!("no hosts in host_config, probing files table");
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT DISTINCT host_id FROM files ORDER BY host_id")
                .fetch_all(db.get_sqlite_connection_pool())
                .await
                .map_err(|e| {
                    S3GalleryError::DbError(format!("Failed to probe files table: {e}"))
                })?;

        if rows.is_empty() {
            return Err(S3GalleryError::NotFound(
                "No data found in database. Run 's3-gallery scan <bucket>' first.".to_string(),
            ));
        }

        for host_id in &rows {
            hosts.push(HostConfigEntry {
                host_id: host_id.clone(),
                host_name: host_id.clone(),
                host_type: "auto".to_string(),
                description: String::new(),
                created_at: String::new(),
                bucket: cli.bucket.clone().unwrap_or_default(),
                endpoint: cli.endpoint.clone(),
                region: cli.region.clone(),
            });
        }
        tracing::info!("discovered {} host(s) from files table", hosts.len());
    }

    // Create S3 client from the first host's config
    let first_host = hosts
        .first()
        .ok_or_else(|| S3GalleryError::Internal("no hosts available".to_string()))?;
    let endpoint = if first_host.endpoint.is_empty() {
        &cli.endpoint
    } else {
        &first_host.endpoint
    };
    let region = if first_host.region.is_empty() {
        &cli.region
    } else {
        &first_host.region
    };
    let config = OssConfig::validate(
        BucketName::new("placeholder")
            .map_err(|_| S3GalleryError::Internal("invalid placeholder".to_string()))?,
        endpoint,
        region,
        &cli.access_key,
        &cli.secret_key,
        10,
    )?;
    let core_s3 = S3Service::new(Arc::new(RealS3Client::from_config(&config)));

    // Traffic recorder with channel-based batch writer
    // spawn_batch_writer needs a raw SqlitePool, so create one separately
    let sqlite_pool = SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.display()))
        .await
        .map_err(|e| {
            S3GalleryError::DbError(format!("Failed to create pool for batch writer: {e}"))
        })?;
    let handle = spawn_batch_writer(sqlite_pool, 5, 100);
    let recorder = Arc::new(TrafficRecorder::new(handle.sender.clone()));

    // Apply layers: LogLayer wraps core_s3
    let s3_stack = LogLayer.layer(core_s3);

    // Register templates from s3-gallery-web
    let mut env = minijinja::Environment::new();
    let errors = s3_gallery_web::register_templates(&mut env);
    if !errors.is_empty() {
        tracing::warn!(
            "{} template(s) failed to register: {:?}",
            errors.len(),
            errors
        );
    }

    // Build AppState
    let app_state = AppState {
        templates: Arc::new(env),
        db,
        hosts: hosts
            .into_iter()
            .map(|h| {
                let host_id = HostId::new(h.host_id)
                    .map_err(|e| S3GalleryError::Internal(format!("Invalid host_id: {e}")))?;
                Ok::<_, S3GalleryError>(host_config::Model {
                    host_id,
                    host_name: h.host_name,
                    host_type: h.host_type,
                    description: h.description,
                    created_at: h.created_at,
                    bucket: h.bucket,
                    endpoint: h.endpoint,
                    region: h.region,
                })
            })
            .collect::<Result<Vec<_>>>()?,
        s3_stack,
        traffic_recorder: Some(recorder),
        prefix,
        cli_region: cli.region.clone(),
        access_key: cli.access_key.clone(),
        secret_key: cli.secret_key.clone(),
    };

    // Create router and start server
    let host_list: Vec<String> = app_state
        .hosts
        .iter()
        .map(|h| h.host_id.to_string())
        .collect();
    let app = create_router(app_state)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("s3-gallery web server starting on {}", addr);
    tracing::info!("  Hosts: {}", host_list.join(", "));
    tracing::info!("  Read-only: {}", readonly);

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| S3GalleryError::Internal(format!("Failed to bind to {addr}: {e}")))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| S3GalleryError::Internal(format!("Server error: {e}")))?;

    Ok(())
}
