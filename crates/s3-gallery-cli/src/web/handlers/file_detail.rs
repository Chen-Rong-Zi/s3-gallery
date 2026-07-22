use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use s3_gallery_core::db::models::{FileEntry, MetadataEntry, ThumbnailEntry};
use s3_gallery_core::error::S3GalleryError;
use serde_json::json;
use std::collections::BTreeMap;

use crate::web::state::AppState;

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
    let template = state.templates.get_template(template_name).map_err(|e| {
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
    tracing::info!(handler = "file_detail", key = %key, "serving file detail");

    // Parse host_id from key (first segment before '/')
    let host_id = match key.find('/') {
        Some(slash) => &key[..slash],
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid key",
                    "detail": "Key must contain host_id/"
                })),
            )
                .into_response();
        }
    };

    // Validate host is known
    if state.get_host(host_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "host not found",
                "detail": format!("No host: {host_id}")
            })),
        )
            .into_response();
    }

    let pool = &state.db;

    // Fetch the file entry by its key.
    let file = match FileEntry::get_by_key(pool, host_id, &key).await {
        Ok(file) => file,
        Err(e) => {
            return match e {
                S3GalleryError::NotFound(_) => {
                    tracing::error!(handler = "file_detail", key = %key, error = %e, "file not found");
                    (
                        StatusCode::NOT_FOUND,
                        Json(json!({
                            "error": "file not found",
                            "detail": format!("No file with key: {key}")
                        })),
                    )
                        .into_response()
                }
                _ => {
                    tracing::error!(handler = "file_detail", key = %key, error = %e, "database error fetching file");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "database error",
                            "detail": e.to_string()
                        })),
                    )
                        .into_response()
                }
            };
        }
    };

    // Fetch metadata entries for this file.
    let metadata_entries = match MetadataEntry::get_by_file_key(pool, &key).await {
        Ok(entries) => entries,
        Err(e) => {
            tracing::error!(handler = "file_detail", key = %key, error = %e, "failed to fetch metadata");
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
    let has_thumbnail = ThumbnailEntry::get(pool, &key).await.is_ok();

    tracing::info!(handler = "file_detail", key = %key, namespaces = %metadata_by_namespace.len(), has_thumbnail = %has_thumbnail, "file detail rendered");

    let file_name = file_name_from_key(&key);
    let file_size_formatted = format_file_size(file.size);

    // Build the template context.
    // Convert BTreeMap to Vec of (namespace, items) pairs so minijinja can
    // iterate with tuple unpacking (BTreeMap serializes to a JSON object,
    // which is not iterable in minijinja).
    let metadata_vec: Vec<(String, Vec<MetadataItem>)> = metadata_by_namespace.into_iter().collect();
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
        "metadata_by_namespace": metadata_vec,
        "has_thumbnail": has_thumbnail,
    });

    match render_template(&state, "file_detail.html", &context) {
        Ok(html) => html.into_response(),
        Err(response) => *response,
    }
}