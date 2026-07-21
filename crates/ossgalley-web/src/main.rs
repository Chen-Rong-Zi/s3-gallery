mod router;
mod state;
mod handlers;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::serve;
use ossgalley_core::s3::client::S3Client;
use ossgalley_core::types::BucketName;
use ossgalley_core::view::LocalView;
use tokio::net::TcpListener;

use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // TODO: Load configuration from config file or environment.
    // "ossgalley" is a valid bucket name (8 lowercase letters).
    let bucket = BucketName::new("ossgalley")?;

    // TODO: Create a real S3 client from config.
    // For now, use a mock client for development.
    let s3_client: Arc<dyn S3Client> = Arc::new(ossgalley_core::s3::mock::MockS3Client::new());

    // TODO: Create or connect to a real database.
    // Use a temporary file for the database to ensure all connections
    // share the same database (unlike :memory: which is per-connection).
    let db_path = std::env::temp_dir().join("ossgalley-web-dev.db");
    let pool = ossgalley_core::db::pool::create_pool(&db_path).await?;
    ossgalley_core::db::schema::run_migrations(&pool).await?;

    let local_view = LocalView::new(pool);

    let app_state = AppState::new(local_view, s3_client, bucket);
    let app = router::create_router(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("ossgalley web server starting on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    serve(listener, app).await?;

    Ok(())
}