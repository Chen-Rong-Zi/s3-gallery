
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::Utc;
use s3_gallery_core::{
    entity::thumbnail,
    error::S3GalleryError,
    thumbnail::generator::generate_thumbnail,
    types::{BucketName, ObjectKey, ThumbnailFormat},
};
use sea_orm::ActiveValue::Set;
use sea_orm::ActiveModelTrait;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;

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

    // Try the database cache first.
    match thumbnail::Entity::find()
        .filter(thumbnail::Column::FileKey.eq(key.as_str()))
        .one(&state.db)
        .await
    {
        Ok(Some(entry)) => {
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
        Ok(None) => {
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
    let mut s3 = state.s3_with_traffic(host_id, "web_thumbnail");

    // Fetch from S3 and generate thumbnail
    match s3.get_object(&bucket_name, &object_key).await {
        Ok(data) => match generate_thumbnail(&data) {
            Ok(thumbnail_data) => {
                // Cache locally
                let file_key = match ObjectKey::new(key.clone()) {
                    Ok(fk) => fk,
                    Err(e) => {
                        tracing::error!(handler = "thumbnail", key = %key, error = %e, "invalid key for caching");
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            [("content-type", "application/json")],
                            format!("{{\"error\":\"invalid key\",\"detail\":\"{e}\"}}").into_bytes(),
                        ).into_response();
                    }
                };
                let _ = thumbnail::ActiveModel {
                    file_key: Set(file_key),
                    data: Set(thumbnail_data.clone()),
                    format: Set(ThumbnailFormat::Jpeg),
                    width: Set(None),
                    height: Set(None),
                    cached_at: Set(Utc::now().to_rfc3339()),
                }
                .insert(&state.db)
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
