use axum::{
    extract::State,
    http::StatusCode,
};
use crate::web::handlers::{HandlerResult, render_template};
use s3_gallery_core::view::tags as tags_view;
use serde_json::json;

use crate::web::state::AppState;

/// Convert a TagEntry to a serde_json::Value for template rendering.
fn tag_entry_to_json(tag: &s3_gallery_core::db::models::TagEntry) -> serde_json::Value {
    json!({
        "tag_id": tag.tag_id,
        "tag_name": tag.tag_name,
        "tag_type": tag.tag_type,
    })
}

/// Tags handler -- renders the tags page.
///
/// Displays all tags with links to search by tag name.
pub async fn tags(State(state): State<AppState>) -> HandlerResult {
    tracing::info!(handler = "tags", "listing tags");

    let tag_list = match tags_view::list_tags(&state.db, None).await {
        Ok(tags) => {
            tracing::info!(handler = "tags", tags = %tags.len(), "tags listed");
            tags
        }
        Err(e) => {
            tracing::error!(handler = "tags", error = %e, "failed to list tags");
            return HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "error": "failed to list tags",
                    "detail": e.to_string()
                }),
            );
        }
    };

    let tags_json: Vec<serde_json::Value> = tag_list.iter().map(tag_entry_to_json).collect();

    let context = json!({ "tags": tags_json });

    render_template(&state, "tags.html", &context)
}
