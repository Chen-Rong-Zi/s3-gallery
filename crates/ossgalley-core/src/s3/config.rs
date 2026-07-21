use chrono::Utc;
use uuid::Uuid;

use crate::error::{OssgalleyError, Result};
use crate::types::{BucketName, ObjectKey};

// ---------------------------------------------------------------------------
// OssConfig
// ---------------------------------------------------------------------------

/// S3-compatible OSS connection configuration.
///
/// Holds the connection parameters needed to talk to an S3-compatible object
/// storage service.  Use [`OssConfig::validate`] to construct a validated
/// instance.
#[derive(Debug, Clone)]
pub struct OssConfig {
    pub bucket: BucketName,
    pub endpoint: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub concurrency: usize,
}

impl OssConfig {
    /// Validate and create an `OssConfig`.
    ///
    /// The following checks are performed:
    /// - `endpoint` must be non-empty and start with `http://` or `https://`
    /// - `region` must be non-empty
    /// - `access_key_id` must be non-empty
    /// - `secret_access_key` must be non-empty
    /// - `concurrency` must be at least 1
    /// - `bucket` is validated via the [`BucketName`] type
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::InvalidConfig` if any validation check fails.
    /// All error messages are joined with `"; "`.
    pub fn validate(
        bucket: BucketName,
        endpoint: &str,
        region: &str,
        access_key_id: &str,
        secret_access_key: &str,
        concurrency: usize,
    ) -> Result<Self> {
        let mut errors: Vec<String> = Vec::new();

        // Validate endpoint
        if endpoint.is_empty() {
            errors.push("endpoint must not be empty".to_string());
        } else if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
            errors.push("endpoint must start with http:// or https://".to_string());
        }

        // Validate region
        if region.is_empty() {
            errors.push("region must not be empty".to_string());
        }

        // Validate access key
        if access_key_id.is_empty() {
            errors.push("access_key_id must not be empty".to_string());
        }

        // Validate secret key
        if secret_access_key.is_empty() {
            errors.push("secret_access_key must not be empty".to_string());
        }

        // Validate concurrency
        if concurrency == 0 {
            errors.push("concurrency must be at least 1".to_string());
        }

        if errors.is_empty() {
            Ok(Self {
                bucket,
                endpoint: endpoint.to_string(),
                region: region.to_string(),
                access_key_id: access_key_id.to_string(),
                secret_access_key: secret_access_key.to_string(),
                concurrency,
            })
        } else {
            Err(OssgalleyError::InvalidConfig(errors.join("; ")))
        }
    }
}

// ---------------------------------------------------------------------------
// HostIdentifier
// ---------------------------------------------------------------------------

/// Identifies a host (a directory prefix in an OSS bucket).
///
/// Derives all `.ossgallery/` paths from the host prefix.  These paths are
/// pre-computed during construction because the prefix is already validated,
/// so the accessor methods are infallible.
#[derive(Debug, Clone)]
pub struct HostIdentifier {
    pub host_id: String,
    pub host_name: String,
    pub host_type: String,
    pub description: String,
    pub bucket: BucketName,
    /// The host directory prefix (e.g., "my-camera").
    pub prefix: ObjectKey,
    #[allow(dead_code)]
    created_at: String,
    #[allow(dead_code)]
    version: u32,
    // Pre-computed paths (guaranteed valid because prefix is validated at
    // construction time).
    db_path: ObjectKey,
    lock_path: ObjectKey,
    config_path: ObjectKey,
    ossgallery_dir: ObjectKey,
}

impl HostIdentifier {
    /// Parse a host string into a `HostIdentifier`.
    ///
    /// The host string is a directory name used as the prefix in the bucket.
    ///
    /// Validation:
    /// - Non-empty input
    /// - No leading `/` (stripped automatically)
    /// - No trailing `/` (stripped automatically)
    /// - No `..` path traversal
    /// - Length <= 200 characters
    ///
    /// A UUID is generated for `host_id`, `"unknown"` is used for `host_type`,
    /// and the current UTC time is used for `created_at`.  The bucket is set
    /// to a placeholder value — callers should replace it with the correct
    /// bucket before use.
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::InvalidHostId` if the host string fails
    /// validation.
    pub fn parse(host_str: &str) -> Result<Self> {
        let prefix = Self::validate_host_str(host_str)?;

        let host_id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();

        // Use a placeholder bucket; callers should set the correct bucket.
        let bucket = BucketName::new("ossgalley")?;

        Self::build(host_id, host_str, "unknown", "", bucket, prefix, created_at, 1)
    }

    /// Create a `HostIdentifier` with an explicit bucket and host prefix.
    ///
    /// The same validation rules as [`parse`](Self::parse) apply to
    /// `host_prefix`.
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::InvalidHostId` if the host prefix fails
    /// validation.
    pub fn new(bucket: BucketName, host_prefix: &str) -> Result<Self> {
        let prefix = Self::validate_host_str(host_prefix)?;

        let host_id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();

        Self::build(host_id, host_prefix, "unknown", "", bucket, prefix, created_at, 1)
    }

    /// Get the path to the DB file: `{prefix}/.ossgallery/ossgallery.db`.
    pub fn db_path(&self) -> &ObjectKey {
        &self.db_path
    }

    /// Get the path to the lock file: `{prefix}/.ossgallery/db.lock`.
    pub fn lock_path(&self) -> &ObjectKey {
        &self.lock_path
    }

    /// Get the path to the config file: `{prefix}/.ossgallery/host.config.json`.
    pub fn config_path(&self) -> &ObjectKey {
        &self.config_path
    }

    /// Get the `.ossgallery` directory path: `{prefix}/.ossgallery/`.
    pub fn ossgallery_dir(&self) -> &ObjectKey {
        &self.ossgallery_dir
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Validate and normalize a host prefix string.
    fn validate_host_str(s: &str) -> Result<ObjectKey> {
        if s.is_empty() {
            return Err(OssgalleyError::InvalidHostId(
                "host string must not be empty".to_string(),
            ));
        }

        let mut normalized = s.to_string();

        // Strip leading slash.
        if let Some(rest) = normalized.strip_prefix('/') {
            normalized = rest.to_string();
        }

        // Strip trailing slash.
        if let Some(rest) = normalized.strip_suffix('/') {
            normalized = rest.to_string();
        }

        if normalized.is_empty() {
            return Err(OssgalleyError::InvalidHostId(
                "host string must not be empty after stripping slashes".to_string(),
            ));
        }

        // Reject path traversal.
        if normalized.contains("..") {
            return Err(OssgalleyError::InvalidHostId(
                "host string must not contain path traversal (..)".to_string(),
            ));
        }

        // Enforce max length.
        if normalized.len() > 200 {
            return Err(OssgalleyError::InvalidHostId(format!(
                "host string must be at most 200 characters, got {}",
                normalized.len()
            )));
        }

        // Validate as an ObjectKey.
        let prefix = ObjectKey::new(normalized)?;

        Ok(prefix)
    }

    /// Construct a `HostIdentifier` and pre-compute all derived paths.
    ///
    /// # Panics
    ///
    /// This function will panic if the derived path strings fail to construct
    /// valid `ObjectKey` values.  This is a programming error because the
    /// prefix is always validated before this method is called.
    ///
    /// The `#[allow(clippy::missing_panics_doc)]` attribute is intentional:
    /// the panic path is a defensive safety net that should never be reached
    /// in practice because the prefix is validated at the call site.
    #[allow(clippy::missing_panics_doc, clippy::too_many_arguments)]
    fn build(
        host_id: String,
        host_name: &str,
        host_type: &str,
        description: &str,
        bucket: BucketName,
        prefix: ObjectKey,
        created_at: String,
        version: u32,
    ) -> Result<Self> {
        let prefix_str = prefix.as_str();

        // The format strings below are guaranteed to produce valid ObjectKeys
        // because prefix is already validated (non-empty, <= 200 chars, no
        // path traversal).  The resulting paths are at most ~215 chars, well
        // under the 1024-character limit.  Defensive construction via
        // ObjectKey::new is used; if it somehow fails, we map the error to an
        // Internal error.
        let db_path = ObjectKey::new(format!("{prefix_str}/.ossgallery/ossgallery.db"))
            .map_err(|e| OssgalleyError::Internal(format!("failed to build db_path: {e}")))?;

        let lock_path = ObjectKey::new(format!("{prefix_str}/.ossgallery/db.lock"))
            .map_err(|e| OssgalleyError::Internal(format!("failed to build lock_path: {e}")))?;

        let config_path = ObjectKey::new(format!("{prefix_str}/.ossgallery/host.config.json"))
            .map_err(|e| OssgalleyError::Internal(format!("failed to build config_path: {e}")))?;

        let ossgallery_dir = ObjectKey::new(format!("{prefix_str}/.ossgallery/"))
            .map_err(|e| OssgalleyError::Internal(format!("failed to build ossgallery_dir: {e}")))?;

        Ok(Self {
            host_id,
            host_name: host_name.to_string(),
            host_type: host_type.to_string(),
            description: description.to_string(),
            bucket,
            prefix,
            created_at,
            version,
            db_path,
            lock_path,
            config_path,
            ossgallery_dir,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- OssConfig tests -----------------------------------------------------

    #[test]
    fn oss_config_valid() -> Result<()> {
        let bucket = BucketName::new("my-bucket")?;
        let config = OssConfig::validate(bucket, "https://s3.amazonaws.com", "us-east-1", "AKID", "secret", 4)?;
        assert_eq!(config.endpoint, "https://s3.amazonaws.com");
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.access_key_id, "AKID");
        assert_eq!(config.secret_access_key, "secret");
        assert_eq!(config.concurrency, 4);
        Ok(())
    }

    #[test]
    fn oss_config_empty_endpoint() {
        let bucket = BucketName::new("my-bucket").ok().unwrap_or_else(|| {
            // Safety: "my-bucket" is a valid bucket name (9 chars, all lowercase).
            // This is a test-only fallback that should never be reached.
            panic!("my-bucket should be valid")
        });
        let err = OssConfig::validate(bucket, "", "us-east-1", "AKID", "secret", 4).unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidConfig(_)));
        assert!(err.to_string().contains("endpoint must not be empty"));
    }

    #[test]
    fn oss_config_invalid_url() {
        let bucket = BucketName::new("my-bucket").ok().unwrap_or_else(|| {
            panic!("my-bucket should be valid")
        });
        let err = OssConfig::validate(bucket, "ftp://bad", "us-east-1", "AKID", "secret", 4).unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidConfig(_)));
        assert!(err.to_string().contains("endpoint must start with http:// or https://"));
    }

    #[test]
    fn oss_config_empty_region() {
        let bucket = BucketName::new("my-bucket").ok().unwrap_or_else(|| {
            panic!("my-bucket should be valid")
        });
        let err = OssConfig::validate(bucket, "https://s3.amazonaws.com", "", "AKID", "secret", 4).unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidConfig(_)));
        assert!(err.to_string().contains("region must not be empty"));
    }

    #[test]
    fn oss_config_empty_access_key() {
        let bucket = BucketName::new("my-bucket").ok().unwrap_or_else(|| {
            panic!("my-bucket should be valid")
        });
        let err = OssConfig::validate(bucket, "https://s3.amazonaws.com", "us-east-1", "", "secret", 4).unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidConfig(_)));
        assert!(err.to_string().contains("access_key_id must not be empty"));
    }

    #[test]
    fn oss_config_empty_secret_key() {
        let bucket = BucketName::new("my-bucket").ok().unwrap_or_else(|| {
            panic!("my-bucket should be valid")
        });
        let err = OssConfig::validate(bucket, "https://s3.amazonaws.com", "us-east-1", "AKID", "", 4).unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidConfig(_)));
        assert!(err.to_string().contains("secret_access_key must not be empty"));
    }

    #[test]
    fn oss_config_zero_concurrency() {
        let bucket = BucketName::new("my-bucket").ok().unwrap_or_else(|| {
            panic!("my-bucket should be valid")
        });
        let err = OssConfig::validate(bucket, "https://s3.amazonaws.com", "us-east-1", "AKID", "secret", 0).unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidConfig(_)));
        assert!(err.to_string().contains("concurrency must be at least 1"));
    }

    #[test]
    fn oss_config_multiple_errors() {
        let bucket = BucketName::new("my-bucket").ok().unwrap_or_else(|| {
            panic!("my-bucket should be valid")
        });
        let err = OssConfig::validate(bucket, "", "", "", "", 0).unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidConfig(_)));
        let msg = err.to_string();
        assert!(msg.contains("endpoint"));
        assert!(msg.contains("region"));
        assert!(msg.contains("access_key_id"));
        assert!(msg.contains("secret_access_key"));
        assert!(msg.contains("concurrency"));
    }

    // -- HostIdentifier tests ------------------------------------------------

    #[test]
    fn host_identifier_parse_valid() -> Result<()> {
        let host = HostIdentifier::parse("my-camera")?;
        assert_eq!(host.prefix.as_str(), "my-camera");
        assert_eq!(host.host_name, "my-camera");
        assert_eq!(host.host_type, "unknown");
        assert_eq!(host.version, 1);
        // host_id should be a valid UUID
        assert!(!host.host_id.is_empty());
        // created_at should be a valid RFC 3339 timestamp
        assert!(!host.created_at.is_empty());
        Ok(())
    }

    #[test]
    fn host_identifier_parse_leading_slash() -> Result<()> {
        let host = HostIdentifier::parse("/my-camera")?;
        assert_eq!(host.prefix.as_str(), "my-camera");
        assert_eq!(host.host_name, "/my-camera");
        Ok(())
    }

    #[test]
    fn host_identifier_parse_trailing_slash() -> Result<()> {
        let host = HostIdentifier::parse("my-camera/")?;
        assert_eq!(host.prefix.as_str(), "my-camera");
        assert_eq!(host.host_name, "my-camera/");
        Ok(())
    }

    #[test]
    fn host_identifier_parse_leading_and_trailing_slash() -> Result<()> {
        let host = HostIdentifier::parse("/my-camera/")?;
        assert_eq!(host.prefix.as_str(), "my-camera");
        Ok(())
    }

    #[test]
    fn host_identifier_parse_empty() {
        let err = HostIdentifier::parse("").unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidHostId(_)));
    }

    #[test]
    fn host_identifier_parse_only_slash() {
        let err = HostIdentifier::parse("/").unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidHostId(_)));
    }

    #[test]
    fn host_identifier_parse_path_traversal() {
        let err = HostIdentifier::parse("my-camera/../other").unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidHostId(_)));
    }

    #[test]
    fn host_identifier_parse_too_long() {
        let long = "a".repeat(201);
        let err = HostIdentifier::parse(&long).unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidHostId(_)));
    }

    #[test]
    fn host_identifier_new_valid() -> Result<()> {
        let bucket = BucketName::new("my-bucket")?;
        let host = HostIdentifier::new(bucket, "my-camera")?;
        assert_eq!(host.prefix.as_str(), "my-camera");
        assert_eq!(host.bucket.as_str(), "my-bucket");
        Ok(())
    }

    #[test]
    fn host_identifier_new_empty_prefix() {
        let bucket = BucketName::new("my-bucket").ok().unwrap_or_else(|| {
            panic!("my-bucket should be valid")
        });
        let err = HostIdentifier::new(bucket, "").unwrap_err();
        assert!(matches!(err, OssgalleyError::InvalidHostId(_)));
    }

    // -- Path derivation tests -----------------------------------------------

    #[test]
    fn host_identifier_db_path() -> Result<()> {
        let host = HostIdentifier::parse("my-camera")?;
        assert_eq!(
            host.db_path().as_str(),
            "my-camera/.ossgallery/ossgallery.db"
        );
        Ok(())
    }

    #[test]
    fn host_identifier_lock_path() -> Result<()> {
        let host = HostIdentifier::parse("my-camera")?;
        assert_eq!(
            host.lock_path().as_str(),
            "my-camera/.ossgallery/db.lock"
        );
        Ok(())
    }

    #[test]
    fn host_identifier_config_path() -> Result<()> {
        let host = HostIdentifier::parse("my-camera")?;
        assert_eq!(
            host.config_path().as_str(),
            "my-camera/.ossgallery/host.config.json"
        );
        Ok(())
    }

    #[test]
    fn host_identifier_ossgallery_dir() -> Result<()> {
        let host = HostIdentifier::parse("my-camera")?;
        assert_eq!(
            host.ossgallery_dir().as_str(),
            "my-camera/.ossgallery/"
        );
        Ok(())
    }

    #[test]
    fn host_identifier_paths_with_nested_prefix() -> Result<()> {
        let host = HostIdentifier::parse("cameras/backyard")?;
        assert_eq!(
            host.db_path().as_str(),
            "cameras/backyard/.ossgallery/ossgallery.db"
        );
        assert_eq!(
            host.lock_path().as_str(),
            "cameras/backyard/.ossgallery/db.lock"
        );
        assert_eq!(
            host.config_path().as_str(),
            "cameras/backyard/.ossgallery/host.config.json"
        );
        assert_eq!(
            host.ossgallery_dir().as_str(),
            "cameras/backyard/.ossgallery/"
        );
        Ok(())
    }

    #[test]
    fn host_identifier_all_paths_are_valid_object_keys() -> Result<()> {
        let host = HostIdentifier::parse("test-host")?;
        // Verify that all paths are valid ObjectKeys by round-tripping through
        // ObjectKey::new.
        let paths = [
            host.db_path(),
            host.lock_path(),
            host.config_path(),
            host.ossgallery_dir(),
        ];
        for path in &paths {
            let reconstructed = ObjectKey::new(path.as_str())?;
            assert_eq!(reconstructed.as_str(), path.as_str());
        }
        Ok(())
    }
}