use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use ossgalley_core::types::{SortField, SortOrder};
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

/// Query parameters for the browse page.
#[derive(Debug, Default, Deserialize)]
pub struct BrowseQuery {
    /// Directory prefix to browse.
    pub path: Option<String>,
    /// Sort field: "name", "size", "last_modified", "file_type".
    pub sort_by: Option<String>,
    /// Sort order: "asc" or "desc".
    pub sort_order: Option<String>,
}

/// A breadcrumb segment in the navigation path.
#[derive(Debug, Clone, serde::Serialize)]
struct Breadcrumb {
    name: String,
    path: String,
}

/// A directory entry with full path, for template rendering.
#[derive(Debug, Clone, serde::Serialize)]
struct EntryView {
    name: String,
    path: String,
    size: String,
    file_type: String,
    last_modified: String,
    is_directory: bool,
}

/// Parse a sort_by query string into a SortField, defaulting to Name.
fn parse_sort_field(s: Option<&str>) -> SortField {
    match s.and_then(|s| s.trim().to_lowercase().parse::<SortField>().ok()) {
        Some(field) => field,
        None => SortField::Name,
    }
}

/// Parse a sort_order query string into a SortOrder.
///
/// Accepts "asc" / "ascending" for ascending, "desc" / "descending" for
/// descending.  Defaults to ascending.
fn parse_sort_order(s: Option<&str>) -> SortOrder {
    match s {
        Some(s) => match s.trim().to_lowercase().as_str() {
            "desc" | "descending" => SortOrder::Descending,
            _ => SortOrder::Ascending,
        },
        None => SortOrder::Ascending,
    }
}

/// Generate breadcrumbs from a path string.
///
/// For example, `"photos/2024/vacation"` produces:
/// ```json
/// [
///   { "name": "photos", "path": "photos" },
///   { "name": "2024", "path": "photos/2024" },
///   { "name": "vacation", "path": "photos/2024/vacation" }
/// ]
/// ```
fn generate_breadcrumbs(path: &str) -> Vec<Breadcrumb> {
    if path.is_empty() {
        return Vec::new();
    }

    let mut crumbs = Vec::new();
    let mut accumulated = String::new();

    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        if !accumulated.is_empty() {
            accumulated.push('/');
        }
        accumulated.push_str(segment);
        crumbs.push(Breadcrumb {
            name: segment.to_string(),
            path: accumulated.clone(),
        });
    }

    crumbs
}

/// Build a context map for template rendering.
fn build_context(
    entries: &[EntryView],
    breadcrumbs: &[Breadcrumb],
    sort_by: &str,
    sort_order: &str,
) -> serde_json::Value {
    let mut ctx = serde_json::Map::new();
    ctx.insert("entries".to_string(), json!(entries));
    ctx.insert("breadcrumbs".to_string(), json!(breadcrumbs));
    ctx.insert("sort_by".to_string(), json!(sort_by));
    ctx.insert("sort_order".to_string(), json!(sort_order));
    serde_json::Value::Object(ctx)
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

/// Browse handler -- renders the directory listing page.
///
/// Query parameters:
/// - `path`: directory prefix to browse (default: "")
/// - `sort_by`: sort field: "name", "size", "last_modified", "file_type" (default: "name")
/// - `sort_order`: "asc" or "desc" (default: "asc")
///
/// If the request includes the `HX-Request` header, only the browse_table.html
/// partial is rendered (for HTMX sorting requests).
pub async fn browse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<BrowseQuery>,
) -> impl IntoResponse {
    let path = params.path.as_deref().unwrap_or("");
    let sort_field = parse_sort_field(params.sort_by.as_deref());
    let sort_order = parse_sort_order(params.sort_order.as_deref());

    // Fetch directory listing from LocalView.
    let entries = match state
        .local_view
        .list_directory(path, sort_field, sort_order)
        .await
    {
        Ok(entries) => entries,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to list directory",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
    };

    // Convert LsEntry to EntryView with full path.
    let effective_prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{}/", path.trim_end_matches('/'))
    };

    let entry_views: Vec<EntryView> = entries
        .iter()
        .map(|entry| {
            let entry_path = if effective_prefix.is_empty() {
                entry.name.clone()
            } else {
                format!("{}{}", effective_prefix, entry.name)
            };
            EntryView {
                name: entry.name.clone(),
                path: entry_path,
                size: entry.size.to_string(),
                file_type: entry.file_type.to_string(),
                last_modified: entry.last_modified.clone(),
                is_directory: entry.is_directory,
            }
        })
        .collect();

    // Generate breadcrumbs.
    let breadcrumbs = generate_breadcrumbs(path);

    let sort_by_str = sort_field.to_string();
    let sort_order_str = if matches!(sort_order, SortOrder::Ascending) {
        "asc"
    } else {
        "desc"
    };

    // Build the template context.
    let context = build_context(&entry_views, &breadcrumbs, &sort_by_str, sort_order_str);

    // Check for HTMX request.
    let is_htmx = headers
        .get("HX-Request")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "true");

    let template_name = if is_htmx { "browse_table.html" } else { "browse.html" };

    match render_template(&state, template_name, &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}