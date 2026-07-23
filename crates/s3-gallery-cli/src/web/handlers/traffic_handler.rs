use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;
use s3_gallery_core::view::traffic::get_traffic_summary;

use crate::web::handlers::{HandlerResult, render_template};
use crate::web::state::AppState;

#[derive(Debug, Deserialize)]
pub struct TrafficQuery {
    pub host_id: Option<String>,
    pub period: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
}

/// Traffic dashboard page — renders traffic.html template.
pub async fn traffic(
    State(state): State<AppState>,
    Query(params): Query<TrafficQuery>,
) -> HandlerResult {
    let summary = match get_traffic_summary(
        &state.db,
        params.host_id.as_deref(),
        params.period.as_deref(),
        params.since.as_deref(),
        params.until.as_deref(),
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            return HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": "failed to get traffic summary", "detail": e.to_string()}),
            );
        }
    };

    let context = json!({
        "businesses": summary.businesses,
        "total_download_mb": format!("{:.1}", summary.total_download_bytes as f64 / 1_048_576.0),
        "total_upload_mb": format!("{:.1}", summary.total_upload_bytes as f64 / 1_048_576.0),
        "total_requests": summary.total_requests,
        "estimated_cost": format!("${:.4}", summary.estimated_cost),
        "top_files": summary.top_files,
    });

    render_template(&state, "traffic.html", &context)
}

/// Live traffic JSON endpoint — polled by HTMX every 5 seconds.
pub async fn traffic_live(
    State(state): State<AppState>,
) -> HandlerResult {
    let row: Result<(i64, i64), _> = sqlx::query_as(
        "SELECT COALESCE(SUM(bytes), 0), COALESCE(SUM(count), 0) \
         FROM traffic_log WHERE recorded_at > datetime('now', '-10 seconds')",
    )
    .fetch_one(&state.db)
    .await;

    match row {
        Ok((bytes, count)) => {
            let rate = bytes as f64 / 10.0;
            HandlerResult::Json(json!({
                "download_bytes_per_sec": rate,
                "download_kbps": format!("{:.1}", rate / 1024.0),
                "requests_per_sec": count as f64 / 10.0,
            }))
        }
        Err(e) => HandlerResult::Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": "failed to read live traffic", "detail": e.to_string()}),
        ),
    }
}

/// Traffic history JSON endpoint.
pub async fn traffic_history(
    State(state): State<AppState>,
    Query(params): Query<TrafficQuery>,
) -> HandlerResult {
    let summary = match get_traffic_summary(
        &state.db,
        params.host_id.as_deref(),
        params.period.as_deref(),
        params.since.as_deref(),
        params.until.as_deref(),
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            return HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": "failed to get traffic history", "detail": e.to_string()}),
            );
        }
    };

    HandlerResult::Json(json!(summary))
}