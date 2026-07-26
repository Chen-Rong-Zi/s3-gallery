use crate::web::handlers::HandlerResult;
use axum::extract::State;

use crate::web::state::AppState;

/// Dashboard handler — shows all hosts from the database.
pub async fn dashboard(State(state): State<AppState>) -> HandlerResult {
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

    HandlerResult::Json(serde_json::json!({
        "status": "ok",
        "service": "s3-gallery-web",
        "hosts": hosts,
        "prefix": state.prefix,
    }))
}
