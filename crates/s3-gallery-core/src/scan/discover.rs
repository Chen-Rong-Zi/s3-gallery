//! DiscoverLayer — discovers hosts from S3, lists objects, writes scan_objects table.
//!
//! This is the innermost pipeline layer, wrapping S3Service directly.
//! It is NOT generic — it wraps S3Service because S3Service is
//! Service<S3Request, Response=S3Response>, not Service<ScanRequest, ...>.
//! The outer layers (DiffLayer, ProcessLayer, AggregateLayer) are generic.

use std::collections::BTreeSet;

use tower::service_fn;
use tower::util::BoxService;
use tower::Layer;
use uuid::Uuid;

use crate::db::models::HostConfigEntry;
use crate::error::S3GalleryError;
use crate::s3::config::HostIdentifier;
use crate::s3::s3_service::S3Service;
use crate::scan::pipeline::{HostInfo, ScanRequest, ScanResponse};
use crate::scan::scan_objects::ScanObjectEntry;
use crate::types::ObjectKey;
use sqlx::SqlitePool;

/// DiscoverLayer wraps S3Service with host discovery logic.
pub struct DiscoverLayer {
    db: SqlitePool,
}

impl DiscoverLayer {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl Layer<S3Service> for DiscoverLayer {
    type Service = BoxService<ScanRequest, ScanResponse, S3GalleryError>;

    fn layer(&self, inner: S3Service) -> Self::Service {
        let db = self.db.clone();
        BoxService::new(service_fn(move |req: ScanRequest| {
            let db = db.clone();
            let endpoint = req.endpoint;
            let bucket = req.bucket;
            let scope_prefix = req.scope_prefix;
            let mut s3 = inner.clone();
            async move {
                let scan_id = Uuid::new_v4().to_string();

                // Determine scope prefix string
                let scope_prefix_str = if scope_prefix.as_str().is_empty() {
                    String::new()
                } else {
                    format!("{}/", scope_prefix.as_str().trim_end_matches('/'))
                };

                // Try to read host.config.json at scope root
                let config_key_str = format!("{}.s3-gallery/host.config.json", scope_prefix_str);
                let config_key = ObjectKey::new(config_key_str.clone())
                    .map_err(|e| S3GalleryError::Internal(format!("Invalid config key: {e}")))?;

                let root_config = s3.get_object(&bucket, &config_key).await.ok();

                let hosts = if let Some(data) = root_config {
                    // CASE 1: Single host at root
                    let host: HostIdentifier = serde_json::from_slice(&data)
                        .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

                    // Save host config
                    HostConfigEntry::upsert_host_config(
                        &db,
                        &host.host_id,
                        &host.host_name,
                        &host.host_type,
                        bucket.as_str(),
                        &endpoint,
                        "",
                    )
                    .await?;

                    let prefix = ObjectKey::new(scope_prefix_str.clone())
                        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                    vec![HostInfo {
                        host_id: host.host_id.clone(),
                        host_name: host.host_name.clone(),
                        prefix,
                        config: Some(host),
                    }]
                } else {
                    // CASE 2: No root config — discover hosts in subdirectories
                    let list_prefix = ObjectKey::new(scope_prefix_str.clone())
                        .map_err(|e| S3GalleryError::Internal(format!("Invalid prefix: {e}")))?;

                    let all_objects = s3.list_objects(&bucket, &list_prefix).await?;

                    // Extract unique first-level directory names
                    let mut subdirs: BTreeSet<String> = BTreeSet::new();
                    for obj in &all_objects {
                        let key = obj.key.as_str();
                        if let Some(rest) = key.strip_prefix(&scope_prefix_str) {
                            if let Some(slash) = rest.find('/') {
                                let dir = &rest[..slash];
                                if !dir.is_empty() {
                                    subdirs.insert(dir.to_string());
                                }
                            }
                        }
                    }

                    let mut discovered = Vec::new();
                    for dir in &subdirs {
                        let dir_prefix_str = format!("{}{}/", scope_prefix_str, dir);
                        let dir_config_key_str =
                            format!("{}{}/.s3-gallery/host.config.json", scope_prefix_str, dir);
                        let dir_config_key = ObjectKey::new(dir_config_key_str)
                            .map_err(|e| S3GalleryError::Internal(format!("Invalid config key: {e}")))?;

                        let dir_config = s3.get_object(&bucket, &dir_config_key).await.ok();

                        if let Some(data) = dir_config {
                            // This subdirectory is a configured host
                            let host: HostIdentifier = serde_json::from_slice(&data)
                                .map_err(|e| S3GalleryError::Internal(format!("Failed to parse host.config.json: {e}")))?;

                            HostConfigEntry::upsert_host_config(
                                &db,
                                &host.host_id,
                                &host.host_name,
                                &host.host_type,
                                bucket.as_str(),
                                &endpoint,
                                "",
                            )
                            .await?;

                            let prefix = ObjectKey::new(dir_prefix_str.clone())
                                .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                            discovered.push(HostInfo {
                                host_id: host.host_id.clone(),
                                host_name: host.host_name.clone(),
                                prefix,
                                config: Some(host),
                            });
                        } else {
                            // No host config — use dir name as host_id
                            HostConfigEntry::upsert_host_config(
                                &db,
                                dir,
                                "unkown",
                                "unkown",
                                bucket.as_str(),
                                &endpoint,
                                "",
                            )
                            .await?;

                            let prefix = ObjectKey::new(dir_prefix_str.clone())
                                .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

                            discovered.push(HostInfo {
                                host_id: dir.clone(),
                                host_name: dir.clone(),
                                prefix,
                                config: None,
                            });
                        }
                    }
                    discovered
                };

                // List objects for each host and write to scan_objects table
                for host in &hosts {
                    let objects = s3.list_objects(&bucket, &host.prefix).await?;

                    // Filter out .s3-gallery directory
                    let filtered: Vec<_> = objects
                        .into_iter()
                        .filter(|obj| {
                            let key = obj.key.as_str();
                            !key.contains("/.s3-gallery/") && !key.starts_with(".s3-gallery/")
                        })
                        .collect();

                    ScanObjectEntry::batch_insert(&db, &scan_id, &host.host_id, &filtered).await?;
                }

                tracing::info!(
                    target: "s3_gallery::scan",
                    scan_id = %scan_id,
                    hosts = hosts.len(),
                    "Discover phase completed"
                );

                Ok(ScanResponse {
                    scan_id,
                    hosts,
                    ..Default::default()
                })
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::create_pool;
    use crate::db::schema::run_migrations;
    use crate::error::Result;
    use crate::s3::mock::MockS3Client;
    use crate::types::BucketName;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tower::Service;

    #[tokio::test]
    async fn test_discover_empty_prefix() -> Result<()> {
        let dir = tempdir().map_err(|e| S3GalleryError::DbError(e.to_string()))?;
        let db_path = dir.path().join("test.db");
        let db = create_pool(&db_path).await?;
        let pool = db.get_sqlite_connection_pool().clone();
        run_migrations(&pool).await?;

        let mock = Arc::new(MockS3Client::new());
        let s3 = S3Service::new(mock);

        let layer = DiscoverLayer::new(pool.clone());
        let mut discover = layer.layer(s3);

        let req = ScanRequest {
            bucket: BucketName::new("test-bucket")?,
            scope_prefix: ObjectKey::new("")?,
            concurrency: 10,
            extract_metadata: false,
            generate_thumbnails: false,
            client_id: "test".to_string(),
            endpoint: String::new(),
        };

        let resp = Service::call(&mut discover, req).await?;
        assert_eq!(resp.hosts.len(), 0);
        assert!(!resp.scan_id.is_empty());

        Ok(())
    }
}
