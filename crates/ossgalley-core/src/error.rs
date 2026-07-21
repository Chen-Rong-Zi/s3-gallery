use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OssgalleyError {
    // --- S3 errors ---
    #[error("S3 operation failed: {0}")]
    S3Error(String),

    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    #[error("Bucket not found: {0}")]
    BucketNotFound(String),

    #[error("Object not found: {0}")]
    ObjectNotFound(String),

    #[error("Key not found in store: {0}")]
    KeyNotFound(String),

    // --- Database errors ---
    #[error("Database error: {0}")]
    DbError(String),

    #[error("Database migration failed: {0}")]
    MigrationError(String),

    // --- Validation errors ---
    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Invalid host identifier: {0}")]
    InvalidHostId(String),

    // --- Metadata errors ---
    #[error("Metadata extraction failed: {0}")]
    MetadataExtraction(String),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    // --- Lock errors ---
    #[error("Failed to acquire lock: {0}")]
    LockAcquisition(String),

    #[error("Lock contention detected: {0}")]
    LockContention(String),

    #[error("Lock not held")]
    LockNotHeld,

    // --- I/O errors ---
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),

    #[error("Thumbnail generation failed: {0}")]
    ThumbnailGeneration(String),

    // --- Generic errors ---
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("Not empty: {0}")]
    NotEmpty(String),
}

pub type Result<T> = std::result::Result<T, OssgalleyError>;