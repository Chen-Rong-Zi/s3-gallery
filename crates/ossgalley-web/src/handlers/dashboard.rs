use axum::{Json, extract::State, response::IntoResponse};
use serde_json::json;

use crate::state::AppState;

/// Dashboard handler.
pub async fn dashboard(_state: State<AppState>) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "ossgalley-web"
    }))
}