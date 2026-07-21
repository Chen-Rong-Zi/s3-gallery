use std::path::PathBuf;
use crate::types::BucketName;
use crate::s3::config::{OssConfig, HostIdentifier};

/// Raw CLI input before validation
pub struct RawCliInput {
    pub endpoint: String,
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    pub bucket: String,
    pub host: String,
    pub db_path: Option<PathBuf>,
    pub concurrency: usize,
    pub readonly: bool,
}

/// CLI-specific options
pub struct CliOptions {
    pub readonly: bool,
    pub concurrency: usize,
}

/// Validated configuration — all fields verified at construction time.
/// Collects all errors before reporting, never panics.
pub struct ValidatedConfig {
    pub oss: OssConfig,
    pub host: HostIdentifier,
    pub db_path: PathBuf,
    pub options: CliOptions,
}

impl ValidatedConfig {
    /// Validate all config fields and collect errors.
    ///
    /// # Errors
    /// Returns a vector of validation error messages if any field fails validation.
    pub fn from_raw(raw: RawCliInput) -> std::result::Result<Self, Vec<String>> {
        let mut errors = Vec::new();

        // Validate bucket (checked first since other validations depend on it)
        let bucket = match BucketName::new(&raw.bucket) {
            Ok(b) => b,
            Err(e) => {
                errors.push(format!("Invalid bucket name: {}", e));
                return Err(errors);
            }
        };

        // Validate host
        let host = match HostIdentifier::new(bucket.clone(), &raw.host) {
            Ok(h) => h,
            Err(e) => {
                errors.push(format!("Invalid host identifier: {}", e));
                return Err(errors);
            }
        };

        // Validate endpoint
        if raw.endpoint.is_empty() {
            errors.push("endpoint must not be empty".to_string());
        } else if !raw.endpoint.starts_with("http://") && !raw.endpoint.starts_with("https://") {
            errors.push("endpoint must start with http:// or https://".to_string());
        }

        // Validate OSS config
        if errors.is_empty() {
            match OssConfig::validate(
                bucket,
                &raw.endpoint,
                &raw.region,
                &raw.access_key,
                &raw.secret_key,
                raw.concurrency,
            ) {
                Ok(oss) => {
                    let db_path = raw.db_path.unwrap_or_else(|| {
                        let mut p = PathBuf::new();
                        p.push(".ossgallery");
                        p.push("ossgallery.db");
                        p
                    });

                    Ok(Self {
                        oss,
                        host,
                        db_path,
                        options: CliOptions {
                            readonly: raw.readonly,
                            concurrency: raw.concurrency,
                        },
                    })
                }
                Err(e) => {
                    errors.push(format!("Invalid OSS config: {}", e));
                    Err(errors)
                }
            }
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_valid_raw() -> RawCliInput {
        RawCliInput {
            endpoint: "http://localhost:9000".to_string(),
            access_key: "s3oss".to_string(),
            secret_key: "s3oss1234".to_string(),
            region: "us-east-1".to_string(),
            bucket: "test-bucket".to_string(),
            host: "test-host".to_string(),
            db_path: None,
            concurrency: 10,
            readonly: false,
        }
    }

    #[test]
    fn test_valid_config() {
        let raw = make_valid_raw();
        let result = ValidatedConfig::from_raw(raw);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_endpoint() {
        let mut raw = make_valid_raw();
        raw.endpoint = "invalid-url".to_string();
        let result = ValidatedConfig::from_raw(raw);
        assert!(result.is_err());
        if let Err(errors) = result {
            assert!(errors.iter().any(|e| e.contains("http")));
        }
    }

    #[test]
    fn test_empty_bucket() {
        let mut raw = make_valid_raw();
        raw.bucket = "".to_string();
        let result = ValidatedConfig::from_raw(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_host() {
        let mut raw = make_valid_raw();
        raw.host = "".to_string();
        let result = ValidatedConfig::from_raw(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_blank_access_key() {
        let mut raw = make_valid_raw();
        raw.access_key = "".to_string();
        let result = ValidatedConfig::from_raw(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_custom_db_path() {
        let mut raw = make_valid_raw();
        raw.db_path = Some(PathBuf::from("/tmp/custom.db"));
        let result = ValidatedConfig::from_raw(raw);
        assert!(result.is_ok());
        if let Ok(config) = result {
            assert_eq!(config.db_path, PathBuf::from("/tmp/custom.db"));
        }
    }
}