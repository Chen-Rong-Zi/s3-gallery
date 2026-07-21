use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use ossgalley_core::{
    db::models::ThumbnailEntry,
    error::OssgalleyError,
    types::ObjectKey,
};

use crate::state::AppState;

/// Thumbnail handler -- serves cached thumbnail images.
///
/// Route: `/thumbnails/{*key}`
///
/// Serves thumbnail images with a 1-year Cache-Control header.
/// Falls back to on-the-fly generation + caching if the thumbnail
/// is not yet in the database.
pub async fn thumbnail(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let pool = state.local_view.db();

    // Try the database cache first.
    match ThumbnailEntry::get(pool, &key).await {
        Ok(entry) => {
            return (
                StatusCode::OK,
                [
                    ("content-type", "image/jpeg"),
                    ("cache-control", "public, max-age=31536000"),
                ],
                entry.data,
            )
                .into_response();
        }
        Err(OssgalleyError::NotFound(_)) => {
            // Not cached — fall through to S3 fetch + generation.
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!(
                    "{{\"error\":\"database error\",\"detail\":\"{}\"}}",
                    e.to_string().replace('"', "\\\"")
                )
                .into_bytes(),
            )
                .into_response();
        }
    }

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

    // Fetch thumbnail via S3 (on-the-fly generation with caching).
    match state.remote_view.fetch_thumbnail(&object_key).await {
        Ok(data) => {
            (
                StatusCode::OK,
                [
                    ("content-type", "image/jpeg"),
                    ("cache-control", "public, max-age=31536000"),
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
                    "{{\"error\":\"thumbnail generation failed\",\"detail\":\"{}\"}}",
                    e.to_string().replace('"', "\\\"")
                )
                .into_bytes(),
            )
                .into_response()
        }
    }
}