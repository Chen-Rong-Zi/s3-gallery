use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use serde_json::json;

use crate::state::AppState;

/// Convert a FileEntry to a serde_json::Value for template rendering.
fn file_entry_to_json(file: &ossgalley_core::db::models::FileEntry) -> serde_json::Value {
    json!({
        "key": file.key,
        "etag": file.etag,
        "size": file.size,
        "last_modified": file.last_modified,
        "content_type": file.content_type,
        "file_type": file.file_type,
        "metadata_state": file.metadata_state,
        "is_deleted": file.is_deleted,
    })
}

/// Convert a TimelineEntry to a serde_json::Value for template rendering.
fn timeline_entry_to_json(
    entry: &ossgalley_core::view::timeline::TimelineEntry,
) -> serde_json::Value {
    let files: Vec<serde_json::Value> = entry.files.iter().map(file_entry_to_json).collect();
    json!({
        "date": entry.date,
        "files": files,
        "count": entry.count,
    })
}

/// Render a minijinja template with the given context.
///
/// # Errors
///
/// Returns an error response if template lookup or rendering fails.
fn render_template(
    state: &AppState,
    template_name: &str,
    context: &serde_json::Value,
) -> Result<Html<String>, Box<Response>> {
    let template = state
        .templates
        .get_template(template_name)
        .map_err(|e| {
            Box::new(
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "template not found",
                        "detail": e.to_string()
                    })),
                )
                    .into_response(),
            )
        })?;

    let html = template.render(context).map_err(|e| {
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "template rendering failed",
                    "detail": e.to_string()
                })),
            )
                .into_response(),
        )
    })?;

    Ok(Html(html))
}

/// Timeline handler -- renders the timeline page.
///
/// Displays files grouped by date, ordered from newest to oldest.
pub async fn timeline(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let timeline_entries = match state.local_view.get_timeline().await {
        Ok(entries) => entries,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to get timeline",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
    };

    let entries_json: Vec<serde_json::Value> =
        timeline_entries.iter().map(timeline_entry_to_json).collect();

    let context = json!({ "timeline": entries_json });

    match render_template(&state, "timeline.html", &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}