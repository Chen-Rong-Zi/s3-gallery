use axum::{
    extract::State,
    http::StatusCode,
};
use crate::web::handlers::{HandlerResult, render_template};
use s3_gallery_core::entity::file;
use s3_gallery_core::view::duplicates as duplicates_view;
use serde_json::json;

use crate::web::state::AppState;

/// Convert a file::Model to a serde_json::Value for template rendering.
fn file_entry_to_json(file: &file::Model) -> serde_json::Value {
    json!({
        "key": file.key,
        "etag": file.etag,
        "size": file.size,
        "last_modified": file.last_modified,
        "content_type": file.content_type,
        "file_type": file.file_type,
        "metadata_state": file.metadata_state,
        "is_deleted": file.is_deleted,
        "host_id": file.host_id,
    })
}

/// Convert a DuplicateGroup to a serde_json::Value for template rendering.
fn duplicate_group_to_json(
    group: &s3_gallery_core::view::duplicates::DuplicateGroup,
) -> serde_json::Value {
    let files: Vec<serde_json::Value> = group.files.iter().map(file_entry_to_json).collect();
    json!({
        "size": group.size.to_string(),
        "files": files,
    })
}

/// Duplicates handler -- renders the duplicates page.
///
/// Displays groups of duplicate files (same size and etag), ordered by
/// size descending.
pub async fn duplicates(State(state): State<AppState>) -> HandlerResult {
    tracing::info!(handler = "duplicates", "finding duplicates");

    let duplicate_groups = match duplicates_view::find_duplicates(&state.db, None).await {
        Ok(groups) => groups,
        Err(e) => {
            tracing::error!(handler = "duplicates", error = %e, "failed to find duplicates");
            return HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "error": "failed to find duplicates",
                    "detail": e.to_string()
                }),
            );
        }
    };

    tracing::info!(handler = "duplicates", groups = %duplicate_groups.len(), "duplicates found");

    let groups_json: Vec<serde_json::Value> = duplicate_groups
        .iter()
        .map(duplicate_group_to_json)
        .collect();

    let context = json!({ "groups": groups_json });

    render_template(&state, "duplicates.html", &context)
}
