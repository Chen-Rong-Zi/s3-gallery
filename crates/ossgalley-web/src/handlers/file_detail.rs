use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use ossgalley_core::db::models::{FileEntry, MetadataEntry, ThumbnailEntry};
use ossgalley_core::error::OssgalleyError;
use serde_json::json;
use std::collections::BTreeMap;

use crate::state::AppState;

/// A single metadata key-value pair for template rendering.
#[derive(Debug, Clone, serde::Serialize)]
struct MetadataItem {
    key: String,
    value: String,
}

/// Format a file size in bytes into a human-readable string.
///
/// Examples: "1.5 KB", "3.2 MB", "1.0 GB"
fn format_file_size(size: i64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size_f = size as f64;
    let mut unit_idx = 0;

    while size_f >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size_f /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", size, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size_f, UNITS[unit_idx])
    }
}

/// Extract the file name from a key (last segment after '/').
fn file_name_from_key(key: &str) -> String {
    match key.rsplit('/').next() {
        Some(name) => name.to_string(),
        None => key.to_string(),
    }
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

/// File detail handler -- renders the file detail page.
///
/// Route: `/files/{*key}`
///
/// Displays file information, metadata grouped by namespace, and a thumbnail
/// preview for the specified file.
pub async fn file_detail(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let pool = state.local_view.db();

    // Fetch the file entry by its key.
    let file = match FileEntry::get_by_key(pool, &key).await {
        Ok(file) => file,
        Err(e) => {
            return match e {
                OssgalleyError::NotFound(_) => (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "file not found",
                        "detail": format!("No file with key: {key}")
                    })),
                )
                    .into_response(),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "database error",
                        "detail": e.to_string()
                    })),
                )
                    .into_response(),
            };
        }
    };

    // Fetch metadata entries for this file.
    let metadata_entries = match MetadataEntry::get_by_file_key(pool, &key).await {
        Ok(entries) => entries,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "failed to fetch metadata",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
    };

    // Group metadata by namespace, preserving alphabetical order.
    let mut metadata_by_namespace: BTreeMap<String, Vec<MetadataItem>> = BTreeMap::new();
    for entry in &metadata_entries {
        let items = metadata_by_namespace
            .entry(entry.namespace.clone())
            .or_default();
        items.push(MetadataItem {
            key: entry.key.clone(),
            value: entry.value.clone(),
        });
    }

    // Check if a thumbnail exists for this file.
    // NotFound is treated as "no thumbnail", not an error.
    let has_thumbnail = ThumbnailEntry::get(pool, &key).await.is_ok();

    let file_name = file_name_from_key(&key);
    let file_size_formatted = format_file_size(file.size);

    // Build the template context.
    let context = json!({
        "file": {
            "key": file.key,
            "etag": file.etag,
            "size": file.size,
            "size_formatted": file_size_formatted,
            "last_modified": file.last_modified,
            "content_type": file.content_type,
            "file_type": file.file_type,
            "metadata_state": file.metadata_state,
            "name": file_name,
        },
        "metadata_by_namespace": metadata_by_namespace,
        "has_thumbnail": has_thumbnail,
    });

    match render_template(&state, "file_detail.html", &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}