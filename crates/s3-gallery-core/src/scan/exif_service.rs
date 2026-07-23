//! ExifService — 下载 64KB EXIF + 提取元数据 + 存储到 DB。
//!
//! 实现 `Service<ExifRequest, Response = ExifResult>`，支持 Clone 以便 BatchService 使用。

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use chrono::Utc;
use tower::Service;

use crate::db::models::MetadataEntry;
use crate::error::{Result, S3GalleryError};
use crate::extractor::exif::ExifExtractor;
use crate::extractor::registry::ExtractorRegistry;
use crate::s3::s3_service::S3Service;
use crate::scan::pipeline::{ExifData, ExifRequest, ExifResult};

/// EXIF 下载 + 提取 + 存储服务。
#[derive(Clone)]
pub struct ExifService {
    s3: S3Service,
    db: sqlx::SqlitePool,
}

impl ExifService {
    pub fn new(s3: S3Service, db: sqlx::SqlitePool) -> Self {
        Self { s3, db }
    }
}

impl Service<ExifRequest> for ExifService {
    type Response = ExifResult;
    type Error = S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ExifRequest) -> Self::Future {
        let mut s3 = self.s3.clone();
        let db = self.db.clone();

        Box::pin(async move {
            // 1. 检查是否有 extractor 支持此文件类型
            let mut registry = ExtractorRegistry::new();
            registry.register(Box::new(ExifExtractor::new()));
            if registry.find(&req.file_type, &req.ext).is_empty() {
                let _ = sqlx::query(
                    "UPDATE files SET metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
                )
                .bind(&req.host_id)
                .bind(req.key.as_str())
                .execute(&db)
                .await;
                return Ok(ExifResult::None);
            }

            // 2. 下载 64KB
            let data = match s3.get_object_range(&req.bucket, &req.key, 0, 65536).await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(key = %req.key, error = %e, "failed to download range for metadata");
                    let _ = sqlx::query(
                        "UPDATE files SET metadata_state = 'failed' WHERE host_id = ? AND key = ?",
                    )
                    .bind(&req.host_id)
                    .bind(req.key.as_str())
                    .execute(&db)
                    .await;
                    return Err(e);
                }
            };

            // 3. 提取元数据
            let items = match registry.extract_all(&data, &req.file_type, &req.ext).await {
                Ok(items) => items,
                Err(e) => {
                    tracing::warn!(key = %req.key, error = %e, "metadata extraction failed");
                    let _ = sqlx::query(
                        "UPDATE files SET metadata_state = 'failed' WHERE host_id = ? AND key = ?",
                    )
                    .bind(&req.host_id)
                    .bind(req.key.as_str())
                    .execute(&db)
                    .await;
                    return Err(S3GalleryError::Internal(e.to_string()));
                }
            };

            if items.is_empty() {
                let _ = sqlx::query(
                    "UPDATE files SET metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
                )
                .bind(&req.host_id)
                .bind(req.key.as_str())
                .execute(&db)
                .await;
                return Ok(ExifResult::None);
            }

            // 4. 存储 MetadataEntry
            let now = Utc::now().to_rfc3339();
            for item in &items {
                MetadataEntry::insert(&db, &MetadataEntry {
                    file_key: req.key.as_str().to_string(),
                    namespace: item.namespace.to_string(),
                    key: item.key.clone(),
                    value: item.value.clone(),
                    extracted_at: now.clone(),
                    partial: false,
                })
                .await?;
            }

            // 5. 计算 effective_date
            let effective_date = items
                .iter()
                .find(|m| m.key == "DateTimeOriginal" || m.key == "DateTimeDigitized")
                .map(|m| m.value.as_str())
                .and_then(|v| v.get(..10))
                .map(|d| d.replace(":", "-"));

            Ok(ExifResult::Some(ExifData {
                items,
                effective_date,
            }))
        })
    }
}