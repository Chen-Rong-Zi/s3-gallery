use std::path::Path;
use chrono::{DateTime, Utc};
use crate::error::{OssgalleyError, Result};
use crate::s3::client::S3Client;
use crate::s3::config::HostIdentifier;

/// Status of the database
#[derive(Debug, Clone, PartialEq)]
pub enum DbStatus {
    /// Local cache is up to date with OSS version
    LocalCacheUpToDate,
    /// OSS has a newer version
    RemoteNewer { remote_modified: String },
    /// DB exists on OSS but not locally
    RemoteOnly,
    /// Neither local nor OSS has a DB
    None,
}

/// Action to take based on DbStatus
#[derive(Debug, Clone, PartialEq)]
pub enum DbAction {
    /// Use local cache, no OSS requests
    UseLocal,
    /// Download DB from OSS (1 GET request)
    DownloadFromOss,
    /// Full scan and upload (acquire lock + scan + PUT DB)
    FullScanAndUpload,
    /// Abort with an error message
    Abort(String),
}

/// Check DB status by examining local cache and OSS state.
/// Uses at most 1 HEAD request (to check if lock exists on OSS).
///
/// # Errors
///
/// Returns an error if:
/// - Failed to access the local cache file
/// - Failed to fetch S3 object metadata for the database
/// - Failed to retrieve the local file's modification time
pub async fn check_db_status(
    cache_path: &Path,
    s3: &dyn S3Client,
    host: &HostIdentifier,
) -> Result<DbStatus> {
    let local_exists = cache_path.exists();
    let db_key = host.db_path();

    // Check if DB exists on OSS (1 HEAD request)
    let remote_modified = match s3.head_object(&host.bucket, db_key).await {
        Ok(meta) => Some(meta.last_modified),
        Err(OssgalleyError::ObjectNotFound(_)) | Err(OssgalleyError::NotFound(_)) => None,
        Err(e) => return Err(e),
    };

    match (local_exists, remote_modified) {
        (true, Some(remote_mtime)) => {
            // Compare local vs remote mtime
            // For simplicity, if remote exists and we have local, check if remote is newer
            // In practice, we'd compare last_modified timestamps
            if let Ok(local_mtime) = get_local_mtime(cache_path) {
                if remote_mtime > local_mtime {
                    Ok(DbStatus::RemoteNewer { remote_modified: remote_mtime })
                } else {
                    Ok(DbStatus::LocalCacheUpToDate)
                }
            } else {
                // Can't read local mtime, treat as remote only
                Ok(DbStatus::RemoteNewer { remote_modified: remote_mtime })
            }
        }
        (false, Some(_remote_mtime)) => {
            Ok(DbStatus::RemoteOnly)
        }
        (true, None) => {
            // Local exists but no remote — local is authoritative
            Ok(DbStatus::LocalCacheUpToDate)
        }
        (false, None) => {
            Ok(DbStatus::None)
        }
    }
}

/// Decide what action to take based on DB status.
/// This is a pure function — no side effects.
pub fn decide_action(status: &DbStatus, readonly: bool) -> DbAction {
    match status {
        DbStatus::LocalCacheUpToDate => DbAction::UseLocal,
        DbStatus::RemoteNewer { .. } => DbAction::DownloadFromOss,
        DbStatus::RemoteOnly => DbAction::DownloadFromOss,
        DbStatus::None if !readonly => DbAction::FullScanAndUpload,
        DbStatus::None => DbAction::Abort(
            "No database found. Run 'ossgalley scan <host>' first.".to_string()
        ),
    }
}

/// Get local file modification time as RFC 3339 string
fn get_local_mtime(path: &Path) -> Result<String> {
    let metadata = std::fs::metadata(path)
        .map_err(OssgalleyError::IoError)?;
    let modified = metadata.modified()
        .map_err(OssgalleyError::IoError)?;
    let dt: DateTime<Utc> = modified.into();
    Ok(dt.to_rfc3339())
}