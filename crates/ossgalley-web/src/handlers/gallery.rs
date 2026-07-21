use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

/// Gallery item view for template rendering.
#[derive(Debug, Clone, serde::Serialize)]
struct GalleryItem {
    key: String,
    name: String,
    thumbnail_url: String,
}

/// Query parameters for the gallery page.
#[derive(Debug, Default, Deserialize)]
pub struct GalleryQuery {
    /// Page number for pagination (0-based, default 0).
    pub page: Option<u32>,
}

/// Image file types to include in the gallery.
const IMAGE_TYPES: &[&str] = &[
    "jpeg", "jpg", "png", "gif", "webp", "bmp", "tiff", "tif", "heic", "heif", "avif",
];

/// Number of items per page.
const ITEMS_PER_PAGE: u32 = 50;

/// Build a context map for template rendering.
fn build_context(items: &[GalleryItem], page: u32, has_more: bool) -> serde_json::Value {
    let mut ctx = serde_json::Map::new();
    ctx.insert("items".to_string(), json!(items));
    ctx.insert("page".to_string(), json!(page));
    ctx.insert("has_more".to_string(), json!(has_more));
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

/// Extract the file name from a key (last segment after '/').
fn file_name_from_key(key: &str) -> String {
    match key.rsplit('/').next() {
        Some(name) => name.to_string(),
        None => key.to_string(),
    }
}

/// Gallery handler -- renders the image gallery page with infinite scroll.
///
/// Query parameters:
/// - `page`: page number for pagination (0-based, default 0)
///
/// If the request includes the `HX-Request` header, only the gallery_items.html
/// partial is rendered (for HTMX infinite scroll requests).
pub async fn gallery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GalleryQuery>,
) -> impl IntoResponse {
    let page = params.page.unwrap_or(0);
    let offset = i64::from(page) * i64::from(ITEMS_PER_PAGE);
    let limit = i64::from(ITEMS_PER_PAGE) + 1; // fetch one extra to detect has_more

    let pool = state.local_view.db();

    // Build the SQL query with a static comma-separated list of placeholders.
    // IMAGE_TYPES is a compile-time constant, so this is safe from injection.
    let placeholders = IMAGE_TYPES.iter().map(|_| "?").collect::<Vec<_>>().join(", ");

    let query_str = format!(
        "SELECT * FROM files WHERE file_type IN ({placeholders}) AND is_deleted = 0 ORDER BY key LIMIT ? OFFSET ?",
    );

    let mut query = sqlx::query_as::<_, ossgalley_core::db::models::FileEntry>(&query_str);

    // Bind each image type parameter.
    for type_str in IMAGE_TYPES {
        query = query.bind(type_str);
    }

    // Bind limit and offset.
    query = query.bind(limit);
    query = query.bind(offset);

    let entries = match query.fetch_all(pool).await {
        Ok(entries) => entries,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to query gallery images",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
    };

    // Determine if there are more pages.
    let has_more = entries.len() > usize::try_from(ITEMS_PER_PAGE).unwrap_or(50);

    // Take only the items for this page (avoid slicing to satisfy indexing lint).
    let page_count = if has_more {
        usize::try_from(ITEMS_PER_PAGE).unwrap_or(50)
    } else {
        entries.len()
    };

    // Convert to GalleryItem views.
    let items: Vec<GalleryItem> = entries
        .iter()
        .take(page_count)
        .map(|entry| {
            let name = file_name_from_key(&entry.key);
            GalleryItem {
                key: entry.key.clone(),
                name,
                thumbnail_url: format!("/thumbnails/{}", entry.key),
            }
        })
        .collect();

    let context = build_context(&items, page, has_more);

    let is_htmx = headers
        .get("HX-Request")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "true");

    let template_name = if is_htmx {
        "gallery_items.html"
    } else {
        "gallery.html"
    };

    match render_template(&state, template_name, &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}