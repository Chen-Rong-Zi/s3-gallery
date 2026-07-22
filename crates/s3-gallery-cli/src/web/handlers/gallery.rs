use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use s3_gallery_core::view::timeline_gallery;
use serde::Deserialize;
use serde_json::json;

use crate::web::state::AppState;

/// Gallery item view for template rendering.
#[derive(Debug, Clone, serde::Serialize)]
struct GalleryItem {
    key: String,
    name: String,
    thumbnail_url: String,
    host_id: String,
    file_type: String,
    size: i64,
    last_modified: String,
}

/// A date group in the timeline gallery.
#[derive(Debug, Clone, serde::Serialize)]
struct TimelineGroup {
    date: String,
    count: u64,
    items: Vec<GalleryItem>,
}

/// Query parameters for the gallery page.
#[derive(Debug, Default, Deserialize)]
pub struct GalleryQuery {
    pub page: Option<u32>,
    pub tag: Option<String>,
    pub host_id: Option<String>,
}

/// Number of date groups per page.
const GROUPS_PER_PAGE: u32 = 10;

/// Build a context map for template rendering.
fn build_context(
    groups: &[TimelineGroup],
    page: u32,
    has_more: bool,
    tag: Option<&str>,
    all_tags: &[serde_json::Value],
) -> serde_json::Value {
    let mut ctx = serde_json::Map::new();
    ctx.insert("groups".to_string(), json!(groups));
    ctx.insert("page".to_string(), json!(page));
    ctx.insert("has_more".to_string(), json!(has_more));
    ctx.insert("tag".to_string(), json!(tag));
    ctx.insert("all_tags".to_string(), json!(all_tags));
    serde_json::Value::Object(ctx)
}

/// Render a minijinja template with the given context.
fn render_template(
    state: &AppState,
    template_name: &str,
    context: &serde_json::Value,
) -> Result<Html<String>, Box<Response>> {
    let template = state.templates.get_template(template_name).map_err(|e| {
        Box::new(
            (StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "template not found", "detail": e.to_string()})),
            ).into_response(),
        )
    })?;
    let html = template.render(context).map_err(|e| {
        Box::new(
            (StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "template rendering failed", "detail": e.to_string()})),
            ).into_response(),
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

/// Gallery handler — renders the unified timeline-gallery page.
pub async fn gallery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GalleryQuery>,
) -> impl IntoResponse {
    let page = params.page.unwrap_or(0);
    let tag = params.tag.as_deref();
    let tag = tag.filter(|t| !t.is_empty());

    tracing::info!(handler = "gallery", page = %page, tag = ?tag, "serving gallery");

    let pool = &state.db;

    let (entries, has_more) = match timeline_gallery::get_timeline_gallery(
        pool, params.host_id.as_deref(), page, GROUPS_PER_PAGE, tag,
    ).await {
        Ok(result) => result,
        Err(e) => {
            tracing::error!(handler = "gallery", error = %e, "failed to get timeline gallery");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "failed to get gallery", "detail": e.to_string()})),
            ).into_response();
        }
    };

    let groups: Vec<TimelineGroup> = entries.iter().map(|entry| {
        let items: Vec<GalleryItem> = entry.files.iter().map(|f| {
            let name = file_name_from_key(&f.key);
            GalleryItem {
                key: f.key.clone(),
                name,
                thumbnail_url: format!("/thumbnails/{}", f.key),
                host_id: f.host_id.clone(),
                file_type: f.file_type.clone(),
                size: f.size,
                last_modified: f.last_modified.clone(),
            }
        }).collect();
        TimelineGroup {
            date: entry.date.clone(),
            count: entry.count,
            items,
        }
    }).collect();

    // Fetch all tags for the filter dropdown
    let all_tags: Vec<serde_json::Value> = match s3_gallery_core::view::tags::list_tags(pool, params.host_id.as_deref()).await {
        Ok(tags) => tags.iter().map(|t| json!({ "name": t.tag_name, "type": t.tag_type })).collect(),
        Err(_) => Vec::new(),
    };

    let context = build_context(&groups, page, has_more, tag, &all_tags);

    let is_htmx = headers
        .get("HX-Request")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "true");

    let template_name = if is_htmx { "gallery_items.html" } else { "gallery.html" };

    match render_template(&state, template_name, &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}