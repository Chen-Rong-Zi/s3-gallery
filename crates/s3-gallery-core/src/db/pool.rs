use std::path::Path;

use sea_orm::{Database, DatabaseConnection};

use crate::error::{Result, S3GalleryError};

/// Create a new SQLite database connection.
///
/// The database file will be created if it does not exist (`mode=rwc`).
///
/// # Errors
///
/// Returns `S3GalleryError::DbError` if the connection cannot be established
/// (e.g. invalid path, permissions, or sqlite incompatibility).
pub async fn create_pool(path: &Path) -> Result<DatabaseConnection> {
    let url = format!("sqlite:{}?mode=rwc", path.display());
    Database::connect(&url)
        .await
        .map_err(|e| S3GalleryError::DbError(format!("Failed to connect to database: {e}")))
}
