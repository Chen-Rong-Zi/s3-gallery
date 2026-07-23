
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use s3_gallery_core::{
    db::models::FileEntry,
    error::S3GalleryError,
    types::{BucketName, ObjectKey},
};

use crate::web::state::AppState;

/// Extract the host_id from the first segment of a key.
/// e.g., "photos/2023/autumn.jpg" -> ("photos", "photos/2023/autumn.jpg")
fn split_host_and_key(key: &str) -> Option<(&str, &str)> {
    let slash = key.find('/')?;
    let host_id = &key[..slash];
    // The full key stored in DB includes the host prefix
    Some((host_id, key))
}

fn file_name_from_key(key: &str) -> String {
    match key.rsplit('/').next() {
        Some(name) => name.to_string(),
        None => key.to_string(),
    }
}

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
pub async fn download(State(state): State<AppState>, Path(key): Path<String>) -> impl IntoResponse {
    tracing::info!(handler = "download", key = %key, "download requested");

    // Parse host_id from key
    let (host_id, full_key) = match split_host_and_key(&key) {
        Some(pair) => pair,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                "{\"error\":\"invalid key\",\"detail\":\"Key must start with host_id/\"}"
                    .to_string()
                    .into_bytes(),
            )
                .into_response();
        }
    };

    // Find the host
    let host = match state.get_host(host_id) {
        Some(h) => h,
        None => {
            return (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!("{{\"error\":\"host not found\",\"detail\":\"No host: {host_id}\"}}")
                    .into_bytes(),
            )
                .into_response();
        }
    };

    let pool = &state.db;

    // Fetch file metadata from the database
    let file = match FileEntry::get_by_key(pool, host_id, full_key).await {
        Ok(f) => f,
        Err(e) => {
            return match e {
                S3GalleryError::NotFound(_) => (
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

    // Validate the object key
    let object_key = match ObjectKey::new(full_key) {
        Ok(k) => k,
        Err(e) => {
            tracing::error!(handler = "download", key = %key, error = %e, "invalid object key");
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                format!("{{\"error\":\"invalid key\",\"detail\":\"{e}\"}}").into_bytes(),
            )
                .into_response();
        }
    };

    // Get the S3 client and bucket for this host
    let bucket_name = match BucketName::new(&host.bucket) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!("{{\"error\":\"invalid bucket\",\"detail\":\"{e}\"}}").into_bytes(),
            )
                .into_response();
        }
    };

    // Build S3 service with traffic recording for "web_download" business
    let mut s3 = state.s3_with_traffic(host_id, "web_download");

    // Fetch file content from S3
    match s3.get_object(&bucket_name, &object_key).await {
        Ok(data) => {
            tracing::info!(handler = "download", key = %key, size = %data.len(), "file downloaded");
            let filename = file_name_from_key(&key);
            let content_type = content_type_for_download(&file);
            let content_length = file.size.to_string();

            (
                StatusCode::OK,
                [
                    ("content-type", content_type),
                    (
                        "content-disposition",
                        &format!("attachment; filename=\"{filename}\""),
                    ),
                    ("content-length", &content_length),
                ],
                data,
            )
                .into_response()
        }
        Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
            tracing::error!(handler = "download", key = %key, "file not found on S3");
            (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!("{{\"error\":\"file not found\",\"detail\":\"No file on S3: {key}\"}}")
                    .into_bytes(),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(handler = "download", key = %key, error = %e, "S3 fetch failed");
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
