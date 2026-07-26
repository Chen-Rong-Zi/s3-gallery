use axum::{
    extract::{Path, State},
    http::StatusCode,
};
use crate::web::handlers::{HandlerResult, render_template};
use s3_gallery_core::entity::file;
use s3_gallery_core::entity::metadata;
use s3_gallery_core::entity::thumbnail;
use s3_gallery_core::error::S3GalleryError;
use s3_gallery_core::types::FileSize;
use sea_orm::ColumnTrait;
use sea_orm::Condition;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
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
fn format_file_size(size: FileSize) -> String {
    format!("{}", size)
}

/// Extract the file name from a key (last segment after '/').
fn file_name_from_key(key: &str) -> String {
    match key.rsplit('/').next() {
        Some(name) => name.to_string(),
        None => key.to_string(),
    }
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
) -> HandlerResult {
    tracing::info!(handler = "file_detail", key = %key, "serving file detail");

    // Parse host_id from key (first segment before '/')
    let host_id = match key.find('/') {
        Some(slash) => &key[..slash],
        None => {
            return HandlerResult::Error(
                StatusCode::BAD_REQUEST,
                json!({
                    "error": "invalid key",
                    "detail": "Key must contain host_id/"
                }),
            );
        }
    };

    // Validate host is known
    if state.get_host(host_id).is_none() {
        return HandlerResult::Error(
            StatusCode::NOT_FOUND,
            json!({
                "error": "host not found",
                "detail": format!("No host: {host_id}")
            }),
        );
    }

    let pool = state.db.get_sqlite_connection_pool();

    // Fetch the file entry by its key.
    let file = match file::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(file::Column::HostId.eq(host_id))
                .add(file::Column::Key.eq(key.as_str())),
        )
        .one(&state.db)
        .await
    {
        Ok(Some(file)) => file,
        Ok(None) => {
            tracing::error!(handler = "file_detail", key = %key, "file not found");
            return HandlerResult::Error(
                StatusCode::NOT_FOUND,
                json!({
                    "error": "file not found",
                    "detail": format!("No file with key: {key}")
                }),
            );
        }
        Err(e) => {
            tracing::error!(handler = "file_detail", key = %key, error = %e, "database error");
            return HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "error": "database error",
                    "detail": e.to_string()
                }),
            );
        }
    };

    // Fetch metadata entries for this file.
    let metadata_entries = match metadata::Entity::find()
        .filter(metadata::Column::FileKey.eq(key.as_str()))
        .all(&state.db)
        .await
    {
        Ok(entries) => entries,
        Err(e) => {
            tracing::error!(handler = "file_detail", key = %key, error = %e, "failed to fetch metadata");
            return HandlerResult::Error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "error": "failed to fetch metadata",
                    "detail": e.to_string()
                }),
            );
        }
    };

    // Group metadata by namespace, preserving alphabetical order.
    let mut metadata_by_namespace: BTreeMap<String, Vec<MetadataItem>> = BTreeMap::new();
    for entry in &metadata_entries {
        let items = metadata_by_namespace
            .entry(entry.namespace.to_string())
            .or_default();
        items.push(MetadataItem {
            key: entry.key.clone(),
            value: entry.value.clone(),
        });
    }

    // Check if a thumbnail exists for this file.
    let has_thumbnail = thumbnail::Entity::find()
        .filter(thumbnail::Column::FileKey.eq(key.as_str()))
        .one(&state.db)
        .await
        .is_ok_and(|r| r.is_some());

    tracing::info!(handler = "file_detail", key = %key, namespaces = %metadata_by_namespace.len(), has_thumbnail = %has_thumbnail, "file detail rendered");

    let file_name = file_name_from_key(&key);
    let file_size_formatted = format_file_size(file.size);

    // Build the template context.
    // Convert BTreeMap to Vec of (namespace, items) pairs so minijinja can
    // iterate with tuple unpacking (BTreeMap serializes to a JSON object,
    // which is not iterable in minijinja).
    let metadata_vec: Vec<(String, Vec<MetadataItem>)> =
        metadata_by_namespace.into_iter().collect();
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

    render_template(&state, "file_detail.html", &context)
}
