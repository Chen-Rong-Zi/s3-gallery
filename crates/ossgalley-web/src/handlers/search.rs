use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

/// Query parameters for the search page.
#[derive(Debug, Default, Deserialize)]
pub struct SearchQuery {
    /// Search query string.
    pub q: Option<String>,
    /// Tag name to search by.
    pub tag: Option<String>,
}

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

/// Search handler -- renders the search page.
///
/// Query parameters:
/// - `q`: search query string (searches by file name)
/// - `tag`: tag name to search by
///
/// If the request includes the `HX-Request` header, only the search_results.html
/// partial is rendered (for HTMX live search).
pub async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let is_htmx = headers
        .get("HX-Request")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "true");

    let files = if let Some(ref query) = params.q {
        let query_trimmed = query.trim();
        if query_trimmed.is_empty() {
            Vec::new()
        } else {
            match state.local_view.search_by_name(query_trimmed).await {
                Ok(result) => result.files,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "search failed",
                            "detail": e.to_string()
                        })),
                    )
                        .into_response();
                }
            }
        }
    } else if let Some(ref tag) = params.tag {
        let tag_trimmed = tag.trim();
        if tag_trimmed.is_empty() {
            Vec::new()
        } else {
            match state.local_view.search_by_tag(tag_trimmed).await {
                Ok(result) => result.files,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "search by tag failed",
                            "detail": e.to_string()
                        })),
                    )
                        .into_response();
                }
            }
        }
    } else {
        Vec::new()
    };

    let results: Vec<serde_json::Value> = files.iter().map(file_entry_to_json).collect();

    let context = json!({ "results": results });

    let template_name = if is_htmx {
        "search_results.html"
    } else {
        "search.html"
    };

    match render_template(&state, template_name, &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}