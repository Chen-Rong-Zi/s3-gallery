use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use ossgalley_core::{
    db::models::FileEntry,
    error::OssgalleyError,
    types::ObjectKey,
};

use crate::state::AppState;

/// Extract the file name from a key (last segment after '/').
fn file_name_from_key(key: &str) -> String {
    match key.rsplit('/').next() {
        Some(name) => name.to_string(),
        None => key.to_string(),
    }
}

/// Determine the content type for the download response.
///
/// Uses the file's stored content type if available, otherwise falls back
/// to `application/octet-stream`.
fn content_type_for_download(file: &FileEntry) -> &str {
    match file.content_type.as_deref() {
        Some(ct) if !ct.is_empty() => ct,
        _ => "application/octet-stream",
    }
}

/// Download handler -- serves a file from S3 for download.
///
/// Route: `/download/{*key}`
///
/// Sets Content-Disposition to force download, along with Content-Type
/// and Content-Length headers.
pub async fn download(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let pool = state.local_view.db();

    // Fetch file metadata from the database to get content type and size.
    let file = match FileEntry::get_by_key(pool, &key).await {
        Ok(f) => f,
        Err(e) => {
            return match e {
                OssgalleyError::NotFound(_) => (
                    StatusCode::NOT_FOUND,
                    [("content-type", "application/json")],
                    format!(
                        "{{\"error\":\"file not found\",\"detail\":\"No file with key: {key}\"}}"
                    )
                    .into_bytes(),
                )
                    .into_response(),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [("content-type", "application/json")],
                    format!(
                        "{{\"error\":\"database error\",\"detail\":\"{}\"}}",
                        e.to_string().replace('"', "\\\"")
                    )
                    .into_bytes(),
                )
                    .into_response(),
            };
        }
    };

    // Validate the object key.
    let object_key = match ObjectKey::new(&key) {
        Ok(k) => k,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"invalid key\",\"detail\":\"{}\"}}",
                    e.to_string().replace('"', "\\\"")
                )
                .into_bytes(),
            )
                .into_response();
        }
    };

    // Fetch file content from S3.
    match state.remote_view.fetch_file_content(&object_key).await {
        Ok(data) => {
            let filename = file_name_from_key(&key);
            let content_type = content_type_for_download(&file);
            let content_length = file.size.to_string();

            (
                StatusCode::OK,
                [
                    ("content-type", content_type),
                    ("content-disposition", &format!("attachment; filename=\"{filename}\"")),
                    ("content-length", &content_length),
                ],
                data,
            )
                .into_response()
        }
        Err(OssgalleyError::ObjectNotFound(_)) | Err(OssgalleyError::NotFound(_)) => {
            (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"file not found\",\"detail\":\"No file with key: {key}\"}}"
                )
                .into_bytes(),
            )
                .into_response()
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"S3 fetch failed\",\"detail\":\"{}\"}}",
                    e.to_string().replace('"', "\\\"")
                )
                .into_bytes(),
            )
                .into_response()
        }
    }
}