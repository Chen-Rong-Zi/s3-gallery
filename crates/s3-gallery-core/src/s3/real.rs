//! Real S3 client implementation that wraps `aws_sdk_s3::Client`.

use async_trait::async_trait;

use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::primitives::ByteStream;

use crate::error::{Result, S3GalleryError};
use crate::s3::client::{ObjectMetadata, ObjectSummary, S3Client};
use crate::s3::config::OssConfig;
use crate::types::{BucketName, Etag, FileSize, ObjectKey};

/// A real S3 client that wraps `aws_sdk_s3::Client` and implements the
/// `S3Client` trait.
pub struct RealS3Client {
    /// The underlying AWS SDK S3 client.
    pub client: aws_sdk_s3::Client,
    /// The bucket name used for all operations.
    pub bucket: BucketName,
}

impl RealS3Client {
    /// Build a new client from an [`OssConfig`].
    pub fn from_config(config: &OssConfig) -> Self {
        let creds = aws_sdk_s3::config::Credentials::new(
            &config.access_key_id,
            &config.secret_access_key,
            None,
            None,
            "s3-gallery",
        );

        let s3_config = aws_sdk_s3::config::Builder::new()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::v2026_01_12())
            .region(aws_sdk_s3::config::Region::new(config.region.clone()))
            .endpoint_url(&config.endpoint)
            .credentials_provider(creds)
            .force_path_style(true)
            .build();

        let client = aws_sdk_s3::Client::from_conf(s3_config);
        Self {
            client,
            bucket: config.bucket.clone(),
        }
    }
}

#[async_trait]
impl S3Client for RealS3Client {
    async fn list_objects(
        &self,
        bucket: &BucketName,
        prefix: &ObjectKey,
    ) -> Result<Vec<ObjectSummary>> {
        let resp = self
            .client
            .list_objects_v2()
            .bucket(bucket.as_str())
            .prefix(prefix.as_str())
            .send()
            .await
            .map_err(|e| {
                if let Some(service_err) = e.as_service_error() {
                    let code = service_err.code().unwrap_or("unknown");
                    let msg = service_err.message().unwrap_or("no message");
                    S3GalleryError::S3Error(format!("{code}: {msg}"))
                } else {
                    S3GalleryError::S3Error(e.to_string())
                }
            })?;

        let mut results = Vec::new();
        if let Some(contents) = resp.contents {
            for obj in contents {
                let key = obj.key().unwrap_or_default();
                if key.is_empty() {
                    continue;
                }
                let etag = obj.e_tag().unwrap_or("unknown").trim_matches('"');
                results.push(ObjectSummary {
                    key: ObjectKey::new(key).map_err(|e| S3GalleryError::S3Error(e.to_string()))?,
                    etag: Etag::new(etag).map_err(|e| S3GalleryError::S3Error(e.to_string()))?,
                    size: FileSize::new(obj.size().unwrap_or(0) as u64),
                    last_modified: obj
                        .last_modified()
                        .map(|d| d.to_string())
                        .unwrap_or_default(),
                });
            }
        }
        Ok(results)
    }

    async fn head_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<ObjectMetadata> {
        let resp = self
            .client
            .head_object()
            .bucket(bucket.as_str())
            .key(key.as_str())
            .send()
            .await
            .map_err(|e| {
                // Check if the error is "Not Found" (404) via the service error code.
                if let Some(service_err) = e.as_service_error() {
                    if service_err.code() == Some("NotFound") {
                        return S3GalleryError::ObjectNotFound(key.to_string());
                    }
                }
                if let Some(service_err) = e.as_service_error() {
                    let code = service_err.code().unwrap_or("unknown");
                    let msg = service_err.message().unwrap_or("no message");
                    S3GalleryError::S3Error(format!("{code}: {msg}"))
                } else {
                    S3GalleryError::S3Error(e.to_string())
                }
            })?;

        let etag = resp.e_tag().unwrap_or("unknown").trim_matches('"');
        Ok(ObjectMetadata {
            etag: Etag::new(etag).map_err(|e| S3GalleryError::S3Error(e.to_string()))?,
            size: FileSize::new(resp.content_length().unwrap_or(0).max(0) as u64),
            content_type: resp.content_type().map(|s| s.to_string()),
            last_modified: resp
                .last_modified()
                .map(|d| d.to_string())
                .unwrap_or_default(),
        })
    }

    async fn get_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<Vec<u8>> {
        let bucket_name = bucket.as_str().to_string();
        let key_str = key.as_str().to_string();
        let resp = self
            .client
            .get_object()
            .bucket(&bucket_name)
            .key(&key_str)
            .send()
            .await
            .map_err(|e| {
                if let Some(service_err) = e.as_service_error() {
                    let code = service_err.code().unwrap_or("unknown");
                    let msg = service_err.message().unwrap_or("no message");
                    S3GalleryError::S3Error(format!(
                        "{code}: {msg} (bucket={bucket_name}, key={key_str})"
                    ))
                } else {
                    S3GalleryError::S3Error(format!("{} (bucket={bucket_name}, key={key_str})", e))
                }
            })?;

        let data = resp
            .body
            .collect()
            .await
            .map_err(|e| S3GalleryError::S3Error(e.to_string()))?;
        Ok(data.to_vec())
    }

    async fn get_object_range(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        start: u64,
        end: u64,
    ) -> Result<Vec<u8>> {
        let range = format!("bytes={}-{}", start, end - 1);
        let resp = self
            .client
            .get_object()
            .bucket(bucket.as_str())
            .key(key.as_str())
            .range(range)
            .send()
            .await
            .map_err(|e| {
                if let Some(service_err) = e.as_service_error() {
                    let code = service_err.code().unwrap_or("unknown");
                    let msg = service_err.message().unwrap_or("no message");
                    S3GalleryError::S3Error(format!("{code}: {msg}"))
                } else {
                    S3GalleryError::S3Error(e.to_string())
                }
            })?;

        let data = resp
            .body
            .collect()
            .await
            .map_err(|e| S3GalleryError::S3Error(e.to_string()))?;
        Ok(data.to_vec())
    }

    async fn put_object(&self, bucket: &BucketName, key: &ObjectKey, body: &[u8]) -> Result<()> {
        self.client
            .put_object()
            .bucket(bucket.as_str())
            .key(key.as_str())
            .body(ByteStream::from(body.to_vec()))
            .send()
            .await
            .map_err(|e| {
                if let Some(service_err) = e.as_service_error() {
                    let code = service_err.code().unwrap_or("unknown");
                    let msg = service_err.message().unwrap_or("no message");
                    S3GalleryError::S3Error(format!("{code}: {msg}"))
                } else {
                    S3GalleryError::S3Error(e.to_string())
                }
            })?;
        Ok(())
    }

    async fn put_object_if_none_match(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        body: &[u8],
    ) -> Result<bool> {
        let result = self
            .client
            .put_object()
            .bucket(bucket.as_str())
            .key(key.as_str())
            .body(ByteStream::from(body.to_vec()))
            .if_none_match("*")
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(err) => {
                // PreconditionFailed (412) means the object already exists.
                if let Some(service_err) = err.as_service_error() {
                    if service_err.code() == Some("PreconditionFailed") {
                        return Ok(false);
                    }
                    let code = service_err.code().unwrap_or("unknown");
                    let msg = service_err.message().unwrap_or("no message");
                    Err(S3GalleryError::S3Error(format!("{code}: {msg}")))
                } else {
                    Err(S3GalleryError::S3Error(err.to_string()))
                }
            }
        }
    }

    async fn delete_object(&self, bucket: &BucketName, key: &ObjectKey) -> Result<()> {
        self.client
            .delete_object()
            .bucket(bucket.as_str())
            .key(key.as_str())
            .send()
            .await
            .map_err(|e| {
                if let Some(service_err) = e.as_service_error() {
                    let code = service_err.code().unwrap_or("unknown");
                    let msg = service_err.message().unwrap_or("no message");
                    S3GalleryError::S3Error(format!("{code}: {msg}"))
                } else {
                    S3GalleryError::S3Error(e.to_string())
                }
            })?;
        Ok(())
    }

    async fn object_exists(&self, bucket: &BucketName, key: &ObjectKey) -> Result<bool> {
        let result = self
            .client
            .head_object()
            .bucket(bucket.as_str())
            .key(key.as_str())
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(err) => {
                // Check if the error is "Not Found" (404) via the service error code.
                if let Some(service_err) = err.as_service_error() {
                    if service_err.code() == Some("NotFound") {
                        return Ok(false);
                    }
                    let code = service_err.code().unwrap_or("unknown");
                    let msg = service_err.message().unwrap_or("no message");
                    return Err(S3GalleryError::S3Error(format!("{code}: {msg}")));
                }
                Err(S3GalleryError::S3Error(err.to_string()))
            }
        }
    }
}
