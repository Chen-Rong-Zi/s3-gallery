pub mod migrate;
pub mod models;
pub mod pool;
pub mod schema;
pub mod status;
pub use self::status::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;

    #[tokio::test]
    async fn test_check_status_no_db() -> Result<()> {
        let dir = tempfile::tempdir()
            .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?;
        let cache_path = dir.path().join("s3-gallery.db");

        let status = check_db_status(&cache_path).await?;
        assert_eq!(status, DbStatus::None);
        Ok(())
    }

    #[tokio::test]
    async fn test_check_status_local_exists() -> Result<()> {
        let dir = tempfile::tempdir()
            .map_err(|e| crate::error::S3GalleryError::DbError(e.to_string()))?;
        let cache_path = dir.path().join("s3-gallery.db");
        std::fs::write(&cache_path, b"test")
            .map_err(|e| crate::error::S3GalleryError::IoError(e))?;

        let status = check_db_status(&cache_path).await?;
        assert_eq!(status, DbStatus::LocalExists);
        Ok(())
    }

    #[tokio::test]
    async fn test_decide_action_use_local() {
        assert_eq!(
            decide_action(&DbStatus::LocalExists, false),
            DbAction::UseLocal
        );
        assert_eq!(
            decide_action(&DbStatus::LocalExists, true),
            DbAction::UseLocal
        );
    }

    #[tokio::test]
    async fn test_decide_action_scan() {
        assert_eq!(decide_action(&DbStatus::None, false), DbAction::FullScan);
    }

    #[tokio::test]
    async fn test_decide_action_abort() {
        assert!(matches!(
            decide_action(&DbStatus::None, true),
            DbAction::Abort(_)
        ));
    }
}
