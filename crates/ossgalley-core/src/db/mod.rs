pub mod models;
pub mod pool;
pub mod schema;
pub mod status;
pub use self::status::*;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::error::{OssgalleyError, Result};
    use crate::s3::mock::MockS3Client;
    use crate::types::BucketName;
    use crate::s3::config::HostIdentifier;

    #[tokio::test]
    async fn test_check_status_no_db() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|e| OssgalleyError::DbError(e.to_string()))?;
        let cache_path = dir.path().join("ossgallery.db");
        let s3 = MockS3Client::new();
        let bucket = BucketName::new("test-bucket")?;
        let host = HostIdentifier::new(bucket, "test-host")?;

        let status = check_db_status(&cache_path, &s3, &host).await?;
        assert_eq!(status, DbStatus::None);
        Ok(())
    }

    #[tokio::test]
    async fn test_decide_action_use_local() {
        assert_eq!(decide_action(&DbStatus::LocalCacheUpToDate, false), DbAction::UseLocal);
        assert_eq!(decide_action(&DbStatus::LocalCacheUpToDate, true), DbAction::UseLocal);
    }

    #[tokio::test]
    async fn test_decide_action_download() {
        assert_eq!(decide_action(&DbStatus::RemoteOnly, false), DbAction::DownloadFromOss);
        assert_eq!(decide_action(&DbStatus::RemoteNewer { remote_modified: "now".to_string() }, false), DbAction::DownloadFromOss);
    }

    #[tokio::test]
    async fn test_decide_action_scan() {
        assert_eq!(decide_action(&DbStatus::None, false), DbAction::FullScanAndUpload);
    }

    #[tokio::test]
    async fn test_decide_action_abort() {
        assert!(matches!(decide_action(&DbStatus::None, true), DbAction::Abort(_)));
    }
}