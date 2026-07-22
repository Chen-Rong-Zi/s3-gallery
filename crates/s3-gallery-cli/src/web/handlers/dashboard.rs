use axum::{extract::State, response::IntoResponse, Json};
use tracing;

use crate::web::state::AppState;

/// Dashboard handler — shows all hosts from the database.
pub async fn dashboard(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("dashboard: rendering");

    let hosts: Vec<_> = state
        .hosts
        .iter()
        .map(|h| {
            serde_json::json!({
                "host_id": h.host_id,
                "bucket": h.bucket,
                "endpoint": h.endpoint,
                "region": h.region,
                "created_at": h.created_at,
            })
        })
        .collect();

    Json(serde_json::json!({
        "status": "ok",
        "service": "s3-gallery-web",
        "hosts": hosts,
        "prefix": state.prefix,
    }))
}
