use crate::cli::Cli;
use s3_gallery_core::error::{Result, S3GalleryError};
use s3_gallery_core::s3::config::{HostIdentifier, OssConfig};
use s3_gallery_core::s3::real::RealS3Client;
use s3_gallery_core::s3::s3_service::S3Service;
use s3_gallery_core::types::{BucketName, ObjectKey};
use std::sync::Arc;

pub async fn run_init(
    cli: &Cli,
    bucket_str: &str,
    prefix: Option<&str>,
    name: &str,
    description: Option<&str>,
) -> Result<()> {
    let bucket = BucketName::new(bucket_str)
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid bucket: {e}")))?;

    // Generate UUID as host_id
    let host_id = uuid::Uuid::new_v4().to_string();

    // Build HostIdentifier with the specified prefix
    let host_prefix = ObjectKey::new(prefix.unwrap_or("").to_string())
        .map_err(|e| S3GalleryError::InvalidConfig(format!("Invalid prefix: {e}")))?;

    let host = HostIdentifier::build(
        host_id,
        name,
        "unknown",
        description.unwrap_or(""),
        bucket.clone(),
        host_prefix,
        chrono::Utc::now().to_rfc3339(),
        1,
    )?;

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&host)
        .map_err(|e| S3GalleryError::Internal(format!("Failed to serialize config: {e}")))?;

    // Create S3 client
    let config = OssConfig::validate(
        bucket.clone(),
        &cli.endpoint,
        &cli.region,
        &cli.access_key,
        &cli.secret_key,
        10,
    )?;
    let mut s3 = S3Service::new(Arc::new(RealS3Client::from_config(&config)));

    // Upload to OSS
    let config_key = host.config_path();
    let json_bytes = json.into_bytes();
    s3.put_object(&bucket, config_key, &json_bytes).await?;

    let prefix_str = prefix.unwrap_or("");
    println!("Host initialized:");
    println!("  host_id: {}", host.host_id);
    println!("  name: {}", host.host_name);
    println!("  bucket: {}", bucket_str);
    println!("  config: {}/.s3-gallery/host.config.json", prefix_str);

    Ok(())
}
