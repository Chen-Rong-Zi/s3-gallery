//! TagService — 从 ExifData 解析标签 + 存储 + 更新 effective_date。
//!
//! 实现 `Service<TagRequest, Response = TagResponse>`，支持 Clone。

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tower::Service;

use crate::error::{Result, S3GalleryError};
use crate::extractor::tag_rules::{evaluate_all, TagRule};
use crate::scan::pipeline::{TagRequest, TagResponse};

/// 标签解析服务。
#[derive(Clone)]
pub struct TagService {
    db: sqlx::SqlitePool,
}

impl TagService {
    pub fn new(db: sqlx::SqlitePool) -> Self {
        Self { db }
    }
}

impl Service<TagRequest> for TagService {
    type Response = TagResponse;
    type Error = S3GalleryError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: TagRequest) -> Self::Future {
        let db = self.db.clone();

        Box::pin(async move {
            let tag_rules = TagRule::default_rules();

            // 1. 生成并存储标签
            let tags = evaluate_all(&tag_rules, &req.exif_data.items, &req.file_type);
            let mut tag_names = Vec::new();
            for tag in &tags {
                sqlx::query("INSERT OR IGNORE INTO tags (tag_name, tag_type) VALUES (?, ?)")
                    .bind(&tag.tag_name)
                    .bind(&tag.tag_type)
                    .execute(&db)
                    .await
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
                let tag_id: i64 = sqlx::query_scalar("SELECT tag_id FROM tags WHERE tag_name = ?")
                    .bind(&tag.tag_name)
                    .fetch_one(&db)
                    .await
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
                sqlx::query("INSERT OR IGNORE INTO file_tags (file_key, tag_id) VALUES (?, ?)")
                    .bind(req.key.to_string())
                    .bind(tag_id)
                    .execute(&db)
                    .await
                    .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
                tag_names.push(tag.tag_name.clone());
            }

            // 2. 添加 exif:yes 标签
            sqlx::query("INSERT OR IGNORE INTO tags (tag_name, tag_type) VALUES (?, ?)")
                .bind("exif:yes")
                .bind("auto")
                .execute(&db)
                .await
                .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
            let exif_tag_id: i64 = sqlx::query_scalar("SELECT tag_id FROM tags WHERE tag_name = ?")
                .bind("exif:yes")
                .fetch_one(&db)
                .await
                .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
            sqlx::query("INSERT OR IGNORE INTO file_tags (file_key, tag_id) VALUES (?, ?)")
                .bind(req.key.to_string())
                .bind(exif_tag_id)
                .execute(&db)
                .await
                .map_err(|e| S3GalleryError::DbError(e.to_string()))?;
            tag_names.push("exif:yes".to_string());

            // 3. 更新 effective_date 和 metadata_state
            let effective_date = req
                .exif_data
                .effective_date
                .as_deref()
                .unwrap_or("")
                .to_string();

            sqlx::query(
                "UPDATE files SET effective_date = ?, metadata_state = 'extracted' WHERE host_id = ? AND key = ?",
            )
            .bind(&effective_date)
            .bind(&req.host_id)
            .bind(&req.key)
            .execute(&db)
            .await
            .map_err(|e| S3GalleryError::DbError(e.to_string()))?;

            Ok(TagResponse { tags: tag_names })
        })
    }
}
