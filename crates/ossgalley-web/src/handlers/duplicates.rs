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

/// Convert a DuplicateGroup to a serde_json::Value for template rendering.
fn duplicate_group_to_json(
    group: &ossgalley_core::view::duplicates::DuplicateGroup,
) -> serde_json::Value {
    let files: Vec<serde_json::Value> = group.files.iter().map(file_entry_to_json).collect();
    json!({
        "size": group.size.to_string(),
        "files": files,
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

/// Duplicates handler -- renders the duplicates page.
///
/// Displays groups of duplicate files (same size and etag), ordered by
/// size descending.
pub async fn duplicates(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let duplicate_groups = match state.local_view.find_duplicates().await {
        Ok(groups) => groups,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to find duplicates",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
    };

    let groups_json: Vec<serde_json::Value> =
        duplicate_groups.iter().map(duplicate_group_to_json).collect();

    let context = json!({ "groups": groups_json });

    match render_template(&state, "duplicates.html", &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}