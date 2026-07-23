use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tower::layer::Layer;

use s3_gallery_core::db::models::HostConfigEntry;
use s3_gallery_core::db::pool::create_pool;
use s3_gallery_core::db::schema::run_migrations;
use s3_gallery_core::db::status::{check_db_status, decide_action, DbAction};
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::config::OssConfig;
use s3_gallery_core::s3::layers::LogLayer;
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::s3::traffic_persist::spawn_aggregator;
use s3_gallery_core::s3::traffic_recorder::TrafficRecorder;
use s3_gallery_core::types::BucketName;

use crate::cli::Cli;
use crate::web::router::create_router;
use crate::web::state::AppState;

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
    let pool = create_pool(db_path).await?;
    run_migrations(&pool).await?;

    // Read all hosts from DB, or discover from files table
    let mut hosts = HostConfigEntry::list_all(&pool).await?;
    if hosts.is_empty() {
        // No host_config entries — probe files table for distinct host_ids
        tracing::info!("no hosts in host_config, probing files table");
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT DISTINCT host_id FROM files ORDER BY host_id")
                .fetch_all(&pool)
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
    let first_host = hosts.first().ok_or_else(|| {
        S3GalleryError::Internal("no hosts available".to_string())
    })?;
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

    // Traffic recorder
    let recorder = Arc::new(TrafficRecorder::new(pool.clone()));
    let _agg_handle = spawn_aggregator(recorder.clone(), pool.clone(), 60);

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
        db: pool,
        hosts,
        s3_stack,
        traffic_recorder: Some(recorder),
        prefix,
        cli_region: cli.region.clone(),
        access_key: cli.access_key.clone(),
        secret_key: cli.secret_key.clone(),
    };

    // Create router and start server
    let host_list: Vec<String> = app_state.hosts.iter().map(|h| h.host_id.clone()).collect();
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
