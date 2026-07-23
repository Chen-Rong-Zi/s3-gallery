use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use crate::web::handlers::{HandlerResult, render_template};
use s3_gallery_core::view::search as search_view;
use serde::Deserialize;
use serde_json::json;

use crate::web::state::AppState;

/// Query parameters for the search page.
#[derive(Debug, Default, Deserialize)]
pub struct SearchQuery {
    /// Search query string.
    pub q: Option<String>,
    /// Tag name to search by.
    pub tag: Option<String>,
    /// Host ID to search within (optional, defaults to first host).
    pub host: Option<String>,
}

/// Convert a FileEntry to a serde_json::Value for template rendering.
fn file_entry_to_json(file: &s3_gallery_core::db::models::FileEntry) -> serde_json::Value {
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
) -> HandlerResult {
    let is_htmx = headers
        .get("HX-Request")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "true");

    let query = params.q.as_deref().unwrap_or("");
    let tag = params.tag.as_deref().unwrap_or("");
    tracing::info!(handler = "search", query = %query, tag = %tag, "search requested");

    // Resolve host_id from optional query param or first host
    let host_id = match params
        .host
        .as_deref()
        .and_then(|h| state.get_host(h).map(|_| h.to_string()))
        .or_else(|| state.hosts.first().map(|h| h.host_id.clone()))
    {
        Some(h) => h,
        None => {
            return HandlerResult::Error(
                StatusCode::NOT_FOUND,
                json!({
                    "error": "no hosts",
                    "detail": "No hosts in database"
                }),
            );
        }
    };

    let pool = &state.db;

    let files = if let Some(ref query) = params.q {
        let query_trimmed = query.trim();
        if query_trimmed.is_empty() {
            Vec::new()
        } else {
            match search_view::search_by_name(pool, &host_id, query_trimmed).await {
                Ok(result) => result.files,
                Err(e) => {
                    tracing::error!(handler = "search", query = %query_trimmed, error = %e, "search by name failed");
                    return HandlerResult::Error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        json!({
                            "error": "search failed",
                            "detail": e.to_string()
                        }),
                    );
                }
            }
        }
    } else if let Some(ref tag) = params.tag {
        let tag_trimmed = tag.trim();
        if tag_trimmed.is_empty() {
            Vec::new()
        } else {
            match search_view::search_by_tag(pool, &host_id, tag_trimmed).await {
                Ok(result) => result.files,
                Err(e) => {
                    tracing::error!(handler = "search", tag = %tag_trimmed, error = %e, "search by tag failed");
                    return HandlerResult::Error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        json!({
                            "error": "search by tag failed",
                            "detail": e.to_string()
                        }),
                    );
                }
            }
        }
    } else {
        Vec::new()
    };

    let results: Vec<serde_json::Value> = files.iter().map(file_entry_to_json).collect();
    tracing::info!(handler = "search", query = %query, tag = %tag, results = %results.len(), "search completed");

    let context = json!({ "results": results });

    let template_name = if is_htmx {
        "search_results.html"
    } else {
        "search.html"
    };

    render_template(&state, template_name, &context)
}
