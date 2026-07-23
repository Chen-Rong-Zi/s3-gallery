use axum::{routing::get, Router};

use crate::web::handlers;
use crate::web::state::AppState;

/// Create the axum Router with shared application state.
pub fn create_router(app_state: AppState) -> Router {
    Router::new()
        .route("/", get(handlers::dashboard))
        .route("/browse", get(handlers::browse))
        .route("/gallery", get(handlers::gallery))
        .route("/search", get(handlers::search))
        .route("/tags", get(handlers::tags))
        .route("/files/*key", get(handlers::file_detail))
        .route("/duplicates", get(handlers::duplicates))
        .route("/stats", get(handlers::stats))
        .route("/settings", get(handlers::settings))
        .route("/thumbnails/*key", get(handlers::thumbnail))
        .route("/download/*key", get(handlers::download))
        .route("/traffic", get(handlers::traffic))
        .route("/api/traffic/live", get(handlers::traffic_live))
        .route("/api/traffic/history", get(handlers::traffic_history))
        .with_state(app_state)
}
