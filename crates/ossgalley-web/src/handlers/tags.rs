use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use serde_json::json;

use crate::state::AppState;

/// Convert a TagEntry to a serde_json::Value for template rendering.
fn tag_entry_to_json(tag: &ossgalley_core::db::models::TagEntry) -> serde_json::Value {
    json!({
        "tag_id": tag.tag_id,
        "tag_name": tag.tag_name,
        "tag_type": tag.tag_type,
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

/// Tags handler -- renders the tags page.
///
/// Displays all tags with links to search by tag name.
pub async fn tags(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let tag_list = match state.local_view.list_tags().await {
        Ok(tags) => tags,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to list tags",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
    };

    let tags_json: Vec<serde_json::Value> = tag_list.iter().map(tag_entry_to_json).collect();

    let context = json!({ "tags": tags_json });

    match render_template(&state, "tags.html", &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}