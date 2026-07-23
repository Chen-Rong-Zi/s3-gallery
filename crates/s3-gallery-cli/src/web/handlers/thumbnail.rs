
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::Utc;
use s3_gallery_core::{
    db::models::ThumbnailEntry,
    error::S3GalleryError,
    s3::s3_service::S3Service,
    s3::layers::TrafficLayer,
    thumbnail::generator::generate_thumbnail,
    types::{BucketName, ObjectKey},
};
use tower::ServiceBuilder;

use crate::web::state::AppState;

fn split_host_and_key(key: &str) -> Option<(&str, &str)> {
    let slash = key.find('/')?;
    let host_id = &key[..slash];
    Some((host_id, key))
}

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
    tracing::info!(handler = "thumbnail", key = %key, "serving thumbnail");

    let pool = &state.db;

    // Try the database cache first.
    match ThumbnailEntry::get(pool, &key).await {
        Ok(entry) => {
            tracing::info!(handler = "thumbnail", key = %key, cache = "hit", "thumbnail served from cache");
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
        Err(S3GalleryError::NotFound(_)) => {
            tracing::info!(handler = "thumbnail", key = %key, cache = "miss", "thumbnail not cached, fetching from S3");
        }
        Err(e) => {
            tracing::error!(handler = "thumbnail", key = %key, error = %e, "database error fetching thumbnail");
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

    // Parse host_id from key
    let (host_id, full_key) = match split_host_and_key(&key) {
        Some(pair) => pair,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                "{\"error\":\"invalid key\"}".to_string().into_bytes(),
            )
                .into_response();
        }
    };

    let host = match state.get_host(host_id) {
        Some(h) => h,
        None => {
            return (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!("{{\"error\":\"host not found\",\"detail\":\"{host_id}\"}}").into_bytes(),
            )
                .into_response();
        }
    };

    // Validate the object key
    let object_key = match ObjectKey::new(full_key) {
        Ok(k) => k,
        Err(e) => {
            tracing::error!(handler = "thumbnail", key = %key, error = %e, "invalid thumbnail key");
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                format!("{{\"error\":\"invalid key\",\"detail\":\"{e}\"}}").into_bytes(),
            )
                .into_response();
        }
    };

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

    // Build S3 service with traffic recording for "web_thumbnail" business
    let raw_client = match state.get_s3_client(host) {
        Some(c) => c.clone(),
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                format!("{{\"error\":\"no S3 client\",\"detail\":\"{host_id}\"}}").into_bytes(),
            )
                .into_response();
        }
    };

    let mut s3 = if let Some(ref recorder) = state.traffic_recorder {
        let core = S3Service::new(raw_client);
        ServiceBuilder::new()
            .layer(TrafficLayer::new(recorder.clone(), host_id, "web_thumbnail"))
            .service(core)
    } else {
        S3Service::new(raw_client)
    };

    // Fetch from S3 and generate thumbnail
    match s3.get_object(&bucket_name, &object_key).await {
        Ok(data) => match generate_thumbnail(&data) {
            Ok(thumbnail_data) => {
                // Cache locally
                let _ = ThumbnailEntry::insert(
                    pool,
                    &ThumbnailEntry {
                        file_key: key.clone(),
                        data: thumbnail_data.clone(),
                        format: "jpeg".to_string(),
                        width: None,
                        height: None,
                        cached_at: Utc::now().to_rfc3339(),
                    },
                )
                .await;

                tracing::info!(handler = "thumbnail", key = %key, cache = "generated", size = %thumbnail_data.len(), "thumbnail generated and cached");
                (
                    StatusCode::OK,
                    [
                        ("content-type", "image/jpeg"),
                        ("cache-control", "public, max-age=31536000"),
                    ],
                    thumbnail_data,
                )
                    .into_response()
            }
            Err(e) => {
                tracing::error!(handler = "thumbnail", key = %key, error = %e, "thumbnail generation failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [("content-type", "application/json")],
                    format!("{{\"error\":\"thumbnail generation failed\",\"detail\":\"{e}\"}}")
                        .into_bytes(),
                )
                    .into_response()
            }
        },
        Err(S3GalleryError::ObjectNotFound(_)) | Err(S3GalleryError::NotFound(_)) => {
            tracing::error!(handler = "thumbnail", key = %key, "thumbnail source not found on S3");
            (
                StatusCode::NOT_FOUND,
                [("content-type", "application/json")],
                format!("{{\"error\":\"file not found\",\"detail\":\"{key}\"}}").into_bytes(),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(handler = "thumbnail", key = %key, error = %e, "thumbnail generation failed");
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
