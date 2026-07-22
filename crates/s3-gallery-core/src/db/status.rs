use std::path::Path;

use crate::error::Result;

/// Status of the local database.
#[derive(Debug, Clone, PartialEq)]
pub enum DbStatus {
    /// Local cache file exists.
    LocalExists,
    /// No database file exists.
    None,
}

/// Action to take based on DbStatus.
#[derive(Debug, Clone, PartialEq)]
pub enum DbAction {
    /// Use local cache.
    UseLocal,
    /// Full scan and upload.
    FullScan,
    /// Abort with an error message.
    Abort(String),
}

/// Check DB status by examining the local cache file.
///
/// # Errors
///
/// Returns an error if the file system cannot be accessed.
pub async fn check_db_status(cache_path: &Path) -> Result<DbStatus> {
    if cache_path.exists() {
        Ok(DbStatus::LocalExists)
    } else {
        Ok(DbStatus::None)
    }
}

/// Decide what action to take based on DB status.
/// This is a pure function — no side effects.
pub fn decide_action(status: &DbStatus, readonly: bool) -> DbAction {
    match status {
        DbStatus::LocalExists => DbAction::UseLocal,
        DbStatus::None if !readonly => DbAction::FullScan,
        DbStatus::None => {
            DbAction::Abort("No database found. Run 's3-gallery scan <host>' first.".to_string())
        }
    }
}
