use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Result, S3GalleryError};

// ---------------------------------------------------------------------------
// BucketName
// ---------------------------------------------------------------------------

/// A validated S3 bucket name.
///
/// Rules: non-empty, 3-63 chars, only lowercase letters, digits, and hyphens.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct BucketName(String);

impl BucketName {
    /// Validate and construct a new `BucketName`.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::ValidationError` if the name is empty, shorter
    /// than 3 characters, longer than 63 characters, or contains characters
    /// other than lowercase letters, digits, and hyphens.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if s.is_empty() {
            return Err(S3GalleryError::ValidationError(
                "bucket name must not be empty".into(),
            ));
        }
        let len = s.len();
        if len < 3 {
            return Err(S3GalleryError::ValidationError(format!(
                "bucket name must be at least 3 characters, got {len}"
            )));
        }
        if len > 63 {
            return Err(S3GalleryError::ValidationError(format!(
                "bucket name must be at most 63 characters, got {len}"
            )));
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(S3GalleryError::ValidationError(format!(
                "bucket name must only contain lowercase letters, digits, and hyphens: {s:?}"
            )));
        }
        Ok(Self(s))
    }

    /// View the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BucketName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for BucketName {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl TryFrom<String> for BucketName {
    type Error = S3GalleryError;

    fn try_from(s: String) -> Result<Self> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// ObjectKey
// ---------------------------------------------------------------------------

/// A validated S3 object key.
///
/// Rules: non-empty, max 1024 characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ObjectKey(String);

impl ObjectKey {
    /// Validate and construct a new `ObjectKey`.
    ///
    /// An empty key is allowed and represents the root prefix.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::ValidationError` if the key is longer than 1024
    /// characters.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        let len = s.len();
        if len > 1024 {
            return Err(S3GalleryError::ValidationError(format!(
                "object key must be at most 1024 characters, got {len}"
            )));
        }
        Ok(Self(s))
    }

    /// View the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the parent directory key (everything before the last `/`),
    /// or `None` if there is no `/` or the parent would be empty.
    pub fn parent(&self) -> Option<ObjectKey> {
        let pos = self.0.rfind('/')?;
        if pos == 0 {
            // Leading slash means parent would be empty — not a valid key.
            return None;
        }
        let parent_str = self.0.get(..pos)?;
        Some(Self(parent_str.to_string()))
    }

    /// Returns the portion after the last `/`, or `None` if the key ends
    /// with `/`.  If there is no `/`, returns the entire key.
    pub fn file_name(&self) -> Option<&str> {
        match self.0.rfind('/') {
            Some(pos) => {
                let after = self.0.get(pos + 1..)?;
                if after.is_empty() {
                    None
                } else {
                    Some(after)
                }
            }
            None => Some(self.0.as_str()),
        }
    }
}

impl fmt::Display for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ObjectKey {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl TryFrom<String> for ObjectKey {
    type Error = S3GalleryError;

    fn try_from(s: String) -> Result<Self> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// Etag
// ---------------------------------------------------------------------------

/// An HTTP ETag, stored without surrounding quotes.
///
/// Validation: non-empty.  Leading/trailing `"` characters are stripped
/// automatically.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Etag(String);

impl Etag {
    /// Validate and construct a new `Etag`.
    ///
    /// Surrounding `"` characters are stripped automatically.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::ValidationError` if the etag is empty (or
    /// becomes empty after stripping quotes).
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let mut s = s.into();
        if s.is_empty() {
            return Err(S3GalleryError::ValidationError(
                "etag must not be empty".into(),
            ));
        }
        // Strip surrounding quotes if present.
        let trimmed = s.trim_matches('"');
        if trimmed.is_empty() {
            return Err(S3GalleryError::ValidationError(
                "etag must not be empty after stripping quotes".into(),
            ));
        }
        if trimmed.len() < s.len() {
            // Quotes were stripped — reassign.
            s = trimmed.to_string();
        }
        Ok(Self(s))
    }

    /// View the underlying string (without surrounding quotes).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Etag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Etag {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl TryFrom<String> for Etag {
    type Error = S3GalleryError;

    fn try_from(s: String) -> Result<Self> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// HostId
// ---------------------------------------------------------------------------

/// A validated host identifier.
///
/// Rules: non-empty, 1-64 chars, only alphanumeric + hyphens + underscores.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct HostId(String);

impl HostId {
    /// Validate and construct a new `HostId`.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::ValidationError` if the id is empty, longer
    /// than 64 characters, or contains characters other than alphanumeric,
    /// hyphens, and underscores.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if s.is_empty() {
            return Err(S3GalleryError::ValidationError(
                "host id must not be empty".into(),
            ));
        }
        let len = s.len();
        if len > 64 {
            return Err(S3GalleryError::ValidationError(format!(
                "host id must be at most 64 characters, got {len}"
            )));
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(S3GalleryError::ValidationError(format!(
                "host id must only contain alphanumeric characters, hyphens, and underscores: {s:?}"
            )));
        }
        Ok(Self(s))
    }

    /// View the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HostId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for HostId {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl TryFrom<String> for HostId {
    type Error = S3GalleryError;

    fn try_from(s: String) -> Result<Self> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// FileSize
// ---------------------------------------------------------------------------

/// A file size in bytes.
///
/// All `u64` values are valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FileSize(u64);

impl FileSize {
    /// Construct a new `FileSize` (infallible).
    pub fn new(size: u64) -> Self {
        Self(size)
    }

    /// Return the underlying byte count.
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Whether the size is zero.
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for FileSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
        let size = self.0;
        if size == 0 {
            return write!(f, "0 B");
        }

        // Determine the unit index using integer division.
        let mut unit_idx = 0u32;
        let mut remaining = size;
        while remaining >= 1024 && (unit_idx as usize) < UNITS.len() - 1 {
            remaining /= 1024;
            unit_idx += 1;
        }

        if unit_idx == 0 {
            write!(f, "{size} B")
        } else {
            let divisor = 1024u64.pow(unit_idx);
            let whole = size / divisor;
            let remainder = size % divisor;
            // Round to nearest tenth.
            let decimal = ((remainder * 10) + (divisor / 2)) / divisor;
            // SAFETY: unit_idx is always < UNITS.len() because the while
            // loop above bounds it to UNITS.len() - 1.
            let unit = UNITS.get(unit_idx as usize).ok_or(fmt::Error)?;
            write!(f, "{}.{} {}", whole, decimal, unit)
        }
    }
}

impl FromStr for FileSize {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(S3GalleryError::ValidationError(
                "file size must not be empty".into(),
            ));
        }
        let val: u64 = trimmed.parse().map_err(|e| {
            S3GalleryError::ValidationError(format!("invalid file size {s:?}: {e}"))
        })?;
        Ok(Self(val))
    }
}

// ---------------------------------------------------------------------------
// FileExtension
// ---------------------------------------------------------------------------

/// A validated file extension (no leading dot, lowercase, alphanumeric only).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct FileExtension(String);

impl FileExtension {
    /// Validate and construct a new `FileExtension`.
    ///
    /// The input must be non-empty, contain only lowercase ASCII alphanumeric
    /// characters, and must not have a leading dot.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::ValidationError` if the extension is empty,
    /// starts with a dot, contains non-alphanumeric characters, or is not
    /// lowercase.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if s.is_empty() {
            return Err(S3GalleryError::ValidationError(
                "file extension must not be empty".into(),
            ));
        }
        if s.starts_with('.') {
            return Err(S3GalleryError::ValidationError(format!(
                "file extension must not start with a dot: {s:?}"
            )));
        }
        if s.chars().any(|c| !c.is_ascii_alphanumeric()) {
            return Err(S3GalleryError::ValidationError(format!(
                "file extension must only contain alphanumeric characters: {s:?}"
            )));
        }
        if s.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(S3GalleryError::ValidationError(format!(
                "file extension must be lowercase: {s:?}"
            )));
        }
        Ok(Self(s))
    }

    /// View the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FileExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for FileExtension {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl TryFrom<String> for FileExtension {
    type Error = S3GalleryError;

    fn try_from(s: String) -> Result<Self> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// FileType
// ---------------------------------------------------------------------------

/// A file type enumeration mapping common extensions to their canonical name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    Jpeg,
    Png,
    Gif,
    WebP,
    Bmp,
    Svg,
    Tiff,
    Mp4,
    Mov,
    Avi,
    Mkv,
    WebM,
    Mp3,
    Flac,
    Wav,
    Ogg,
    Aac,
    M4a,
    Pdf,
    Doc,
    Docx,
    Xls,
    Xlsx,
    Ppt,
    Pptx,
    Zip,
    Rar,
    TarGz,
    SevenZ,
    Unknown,
}

impl FileType {
    /// Returns the [`FileCategory`] that this file type belongs to.
    pub fn category(&self) -> FileCategory {
        match self {
            Self::Jpeg
            | Self::Png
            | Self::Gif
            | Self::WebP
            | Self::Bmp
            | Self::Svg
            | Self::Tiff => FileCategory::Image,
            Self::Mp4 | Self::Mov | Self::Avi | Self::Mkv | Self::WebM => FileCategory::Video,
            Self::Mp3 | Self::Flac | Self::Wav | Self::Ogg | Self::Aac | Self::M4a => {
                FileCategory::Audio
            }
            Self::Pdf
            | Self::Doc
            | Self::Docx
            | Self::Xls
            | Self::Xlsx
            | Self::Ppt
            | Self::Pptx => FileCategory::Document,
            Self::Zip | Self::Rar | Self::TarGz | Self::SevenZ => FileCategory::Archive,
            Self::Unknown => FileCategory::Other,
        }
    }
}

impl fmt::Display for FileType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Jpeg => write!(f, "jpeg"),
            Self::Png => write!(f, "png"),
            Self::Gif => write!(f, "gif"),
            Self::WebP => write!(f, "webp"),
            Self::Bmp => write!(f, "bmp"),
            Self::Svg => write!(f, "svg"),
            Self::Tiff => write!(f, "tiff"),
            Self::Mp4 => write!(f, "mp4"),
            Self::Mov => write!(f, "mov"),
            Self::Avi => write!(f, "avi"),
            Self::Mkv => write!(f, "mkv"),
            Self::WebM => write!(f, "webm"),
            Self::Mp3 => write!(f, "mp3"),
            Self::Flac => write!(f, "flac"),
            Self::Wav => write!(f, "wav"),
            Self::Ogg => write!(f, "ogg"),
            Self::Aac => write!(f, "aac"),
            Self::M4a => write!(f, "m4a"),
            Self::Pdf => write!(f, "pdf"),
            Self::Doc => write!(f, "doc"),
            Self::Docx => write!(f, "docx"),
            Self::Xls => write!(f, "xls"),
            Self::Xlsx => write!(f, "xlsx"),
            Self::Ppt => write!(f, "ppt"),
            Self::Pptx => write!(f, "pptx"),
            Self::Zip => write!(f, "zip"),
            Self::Rar => write!(f, "rar"),
            Self::TarGz => write!(f, "tar_gz"),
            Self::SevenZ => write!(f, "seven_z"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl FromStr for FileType {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "jpeg" => Ok(Self::Jpeg),
            "png" => Ok(Self::Png),
            "gif" => Ok(Self::Gif),
            "webp" => Ok(Self::WebP),
            "bmp" => Ok(Self::Bmp),
            "svg" => Ok(Self::Svg),
            "tiff" => Ok(Self::Tiff),
            "mp4" => Ok(Self::Mp4),
            "mov" => Ok(Self::Mov),
            "avi" => Ok(Self::Avi),
            "mkv" => Ok(Self::Mkv),
            "webm" => Ok(Self::WebM),
            "mp3" => Ok(Self::Mp3),
            "flac" => Ok(Self::Flac),
            "wav" => Ok(Self::Wav),
            "ogg" => Ok(Self::Ogg),
            "aac" => Ok(Self::Aac),
            "m4a" => Ok(Self::M4a),
            "pdf" => Ok(Self::Pdf),
            "doc" => Ok(Self::Doc),
            "docx" => Ok(Self::Docx),
            "xls" => Ok(Self::Xls),
            "xlsx" => Ok(Self::Xlsx),
            "ppt" => Ok(Self::Ppt),
            "pptx" => Ok(Self::Pptx),
            "zip" => Ok(Self::Zip),
            "rar" => Ok(Self::Rar),
            "tar_gz" => Ok(Self::TarGz),
            "seven_z" => Ok(Self::SevenZ),
            _ => Ok(Self::Unknown),
        }
    }
}

// ---------------------------------------------------------------------------
// FileCategory
// ---------------------------------------------------------------------------

/// High-level category that a file type belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCategory {
    Image,
    Video,
    Audio,
    Document,
    Archive,
    Other,
}

impl fmt::Display for FileCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Image => write!(f, "image"),
            Self::Video => write!(f, "video"),
            Self::Audio => write!(f, "audio"),
            Self::Document => write!(f, "document"),
            Self::Archive => write!(f, "archive"),
            Self::Other => write!(f, "other"),
        }
    }
}

impl FromStr for FileCategory {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "image" => Ok(Self::Image),
            "video" => Ok(Self::Video),
            "audio" => Ok(Self::Audio),
            "document" => Ok(Self::Document),
            "archive" => Ok(Self::Archive),
            "other" => Ok(Self::Other),
            _ => Err(S3GalleryError::ValidationError(format!(
                "invalid file category: {s:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// MetadataNamespace
// ---------------------------------------------------------------------------

/// A metadata namespace, either a standard category or a custom string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataNamespace {
    Exif,
    Video,
    Audio,
    General,
    Custom(String),
}

impl fmt::Display for MetadataNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exif => write!(f, "exif"),
            Self::Video => write!(f, "video"),
            Self::Audio => write!(f, "audio"),
            Self::General => write!(f, "general"),
            Self::Custom(s) => write!(f, "{s}"),
        }
    }
}

impl FromStr for MetadataNamespace {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "exif" => Ok(Self::Exif),
            "video" => Ok(Self::Video),
            "audio" => Ok(Self::Audio),
            "general" => Ok(Self::General),
            _ => Err(S3GalleryError::ValidationError(format!(
                "invalid metadata namespace: {s:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// MetadataState
// ---------------------------------------------------------------------------

/// The state of metadata extraction for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataState {
    Pending,
    Extracted,
    Failed,
}

impl fmt::Display for MetadataState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Extracted => write!(f, "extracted"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for MetadataState {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "extracted" => Ok(Self::Extracted),
            "failed" => Ok(Self::Failed),
            _ => Err(S3GalleryError::ValidationError(format!(
                "invalid metadata state: {s:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// SyncStatus
// ---------------------------------------------------------------------------

/// The sync status of a file relative to the remote storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Synced,
    Modified,
    New,
    Deleted,
}

impl fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Synced => write!(f, "synced"),
            Self::Modified => write!(f, "modified"),
            Self::New => write!(f, "new"),
            Self::Deleted => write!(f, "deleted"),
        }
    }
}

impl FromStr for SyncStatus {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "synced" => Ok(Self::Synced),
            "modified" => Ok(Self::Modified),
            "new" => Ok(Self::New),
            "deleted" => Ok(Self::Deleted),
            _ => Err(S3GalleryError::ValidationError(format!(
                "invalid sync status: {s:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// DbStatus
// ---------------------------------------------------------------------------

/// The status of a local database or cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbStatus {
    NotCreated,
    Stale,
    Fresh,
    Locked,
    Corrupted,
}

impl fmt::Display for DbStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotCreated => write!(f, "not_created"),
            Self::Stale => write!(f, "stale"),
            Self::Fresh => write!(f, "fresh"),
            Self::Locked => write!(f, "locked"),
            Self::Corrupted => write!(f, "corrupted"),
        }
    }
}

impl FromStr for DbStatus {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "not_created" => Ok(Self::NotCreated),
            "stale" => Ok(Self::Stale),
            "fresh" => Ok(Self::Fresh),
            "locked" => Ok(Self::Locked),
            "corrupted" => Ok(Self::Corrupted),
            _ => Err(S3GalleryError::ValidationError(format!(
                "invalid db status: {s:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// DbAction
// ---------------------------------------------------------------------------

/// An action to perform on a database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbAction {
    Create,
    Download,
    Reuse,
    Recover,
    Wait,
}

impl fmt::Display for DbAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create => write!(f, "create"),
            Self::Download => write!(f, "download"),
            Self::Reuse => write!(f, "reuse"),
            Self::Recover => write!(f, "recover"),
            Self::Wait => write!(f, "wait"),
        }
    }
}

impl FromStr for DbAction {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "create" => Ok(Self::Create),
            "download" => Ok(Self::Download),
            "reuse" => Ok(Self::Reuse),
            "recover" => Ok(Self::Recover),
            "wait" => Ok(Self::Wait),
            _ => Err(S3GalleryError::ValidationError(format!(
                "invalid db action: {s:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// SortField
// ---------------------------------------------------------------------------

/// The field by which to sort a collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    Name,
    Size,
    LastModified,
    FileType,
}

impl fmt::Display for SortField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name => write!(f, "name"),
            Self::Size => write!(f, "size"),
            Self::LastModified => write!(f, "last_modified"),
            Self::FileType => write!(f, "file_type"),
        }
    }
}

impl FromStr for SortField {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "name" => Ok(Self::Name),
            "size" => Ok(Self::Size),
            "last_modified" => Ok(Self::LastModified),
            "file_type" => Ok(Self::FileType),
            _ => Err(S3GalleryError::ValidationError(format!(
                "invalid sort field: {s:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// SortOrder
// ---------------------------------------------------------------------------

/// The direction of a sort operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl fmt::Display for SortOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ascending => write!(f, "ascending"),
            Self::Descending => write!(f, "descending"),
        }
    }
}

impl FromStr for SortOrder {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "ascending" => Ok(Self::Ascending),
            "descending" => Ok(Self::Descending),
            _ => Err(S3GalleryError::ValidationError(format!(
                "invalid sort order: {s:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// ViewMode
// ---------------------------------------------------------------------------

/// The display mode for a file listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    List,
    Grid,
    Timeline,
}

impl fmt::Display for ViewMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List => write!(f, "list"),
            Self::Grid => write!(f, "grid"),
            Self::Timeline => write!(f, "timeline"),
        }
    }
}

impl FromStr for ViewMode {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "list" => Ok(Self::List),
            "grid" => Ok(Self::Grid),
            "timeline" => Ok(Self::Timeline),
            _ => Err(S3GalleryError::ValidationError(format!(
                "invalid view mode: {s:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// ScanMode
// ---------------------------------------------------------------------------

/// The mode for a file-system scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanMode {
    Full,
    Incremental,
}

impl fmt::Display for ScanMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => write!(f, "full"),
            Self::Incremental => write!(f, "incremental"),
        }
    }
}

impl FromStr for ScanMode {
    type Err = S3GalleryError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "full" => Ok(Self::Full),
            "incremental" => Ok(Self::Incremental),
            _ => Err(S3GalleryError::ValidationError(format!(
                "invalid scan mode: {s:?}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- BucketName ---------------------------------------------------------

    #[test]
    fn bucket_name_valid() -> Result<()> {
        let name = BucketName::new("my-bucket")?;
        assert_eq!(name.as_str(), "my-bucket");
        Ok(())
    }

    #[test]
    fn bucket_name_too_short() {
        let err = BucketName::new("ab").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn bucket_name_too_long() {
        let long = "a".repeat(64);
        let err = BucketName::new(long).unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn bucket_name_invalid_chars() {
        let err = BucketName::new("UPPERCASE").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn bucket_name_empty() {
        let err = BucketName::new("").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn bucket_name_serde_roundtrip() -> Result<()> {
        let name = BucketName::new("my-bucket")?;
        let json =
            serde_json::to_string(&name).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        let deserialized: BucketName =
            serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        assert_eq!(name, deserialized);
        Ok(())
    }

    #[test]
    fn bucket_name_display() -> Result<()> {
        let name = BucketName::new("my-bucket")?;
        assert_eq!(format!("{name}"), "my-bucket");
        Ok(())
    }

    // -- ObjectKey ----------------------------------------------------------

    #[test]
    fn object_key_valid() -> Result<()> {
        let key = ObjectKey::new("photos/2024/vacation.jpg")?;
        assert_eq!(key.as_str(), "photos/2024/vacation.jpg");
        Ok(())
    }

    #[test]
    fn object_key_empty() {
        let key = ObjectKey::new("").unwrap();
        assert_eq!(key.as_str(), "");
    }

    #[test]
    fn object_key_too_long() {
        let long = "a".repeat(1025);
        let err = ObjectKey::new(long).unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn object_key_parent_nested() -> Result<()> {
        let key = ObjectKey::new("a/b/c")?;
        let parent = key.parent();
        assert!(parent.is_some());
        assert_eq!(parent.unwrap().as_str(), "a/b");
        Ok(())
    }

    #[test]
    fn object_key_parent_single_level() -> Result<()> {
        let key = ObjectKey::new("a/b")?;
        let parent = key.parent();
        assert!(parent.is_some());
        assert_eq!(parent.unwrap().as_str(), "a");
        Ok(())
    }

    #[test]
    fn object_key_parent_no_slash() -> Result<()> {
        let key = ObjectKey::new("file.txt")?;
        assert!(key.parent().is_none());
        Ok(())
    }

    #[test]
    fn object_key_file_name_nested() -> Result<()> {
        let key = ObjectKey::new("a/b/c")?;
        assert_eq!(key.file_name(), Some("c"));
        Ok(())
    }

    #[test]
    fn object_key_file_name_no_slash() -> Result<()> {
        let key = ObjectKey::new("file.txt")?;
        assert_eq!(key.file_name(), Some("file.txt"));
        Ok(())
    }

    #[test]
    fn object_key_file_name_ends_with_slash() -> Result<()> {
        let key = ObjectKey::new("a/b/")?;
        assert!(key.file_name().is_none());
        Ok(())
    }

    #[test]
    fn object_key_serde_roundtrip() -> Result<()> {
        let key = ObjectKey::new("photos/2024/vacation.jpg")?;
        let json =
            serde_json::to_string(&key).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        let deserialized: ObjectKey =
            serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        assert_eq!(key, deserialized);
        Ok(())
    }

    #[test]
    fn object_key_display() -> Result<()> {
        let key = ObjectKey::new("photos/2024/vacation.jpg")?;
        assert_eq!(format!("{key}"), "photos/2024/vacation.jpg");
        Ok(())
    }

    // -- Etag ---------------------------------------------------------------

    #[test]
    fn etag_valid() -> Result<()> {
        let etag = Etag::new("abc123")?;
        assert_eq!(etag.as_str(), "abc123");
        Ok(())
    }

    #[test]
    fn etag_strips_quotes() -> Result<()> {
        let etag = Etag::new("\"abc123\"")?;
        assert_eq!(etag.as_str(), "abc123");
        Ok(())
    }

    #[test]
    fn etag_empty() {
        let err = Etag::new("").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn etag_only_quotes() {
        let err = Etag::new("\"\"\"").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn etag_serde_roundtrip() -> Result<()> {
        let etag = Etag::new("abc123")?;
        let json =
            serde_json::to_string(&etag).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        let deserialized: Etag =
            serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        assert_eq!(etag, deserialized);
        Ok(())
    }

    #[test]
    fn etag_serde_roundtrip_with_quotes() -> Result<()> {
        // When deserializing a JSON string that contains quotes, they should
        // be stripped.
        let etag: Etag = serde_json::from_str("\"\\\"abc123\\\"\"")
            .map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        assert_eq!(etag.as_str(), "abc123");
        Ok(())
    }

    // -- HostId -------------------------------------------------------------

    #[test]
    fn host_id_valid() -> Result<()> {
        let host = HostId::new("host-01_abc")?;
        assert_eq!(host.as_str(), "host-01_abc");
        Ok(())
    }

    #[test]
    fn host_id_empty() {
        let err = HostId::new("").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn host_id_too_long() {
        let long = "a".repeat(65);
        let err = HostId::new(long).unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn host_id_invalid_chars() {
        let err = HostId::new("host.name").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn host_id_serde_roundtrip() -> Result<()> {
        let host = HostId::new("host-01_abc")?;
        let json =
            serde_json::to_string(&host).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        let deserialized: HostId =
            serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        assert_eq!(host, deserialized);
        Ok(())
    }

    #[test]
    fn host_id_display() -> Result<()> {
        let host = HostId::new("host-01_abc")?;
        assert_eq!(format!("{host}"), "host-01_abc");
        Ok(())
    }

    // -- FileSize -----------------------------------------------------------

    #[test]
    fn file_size_construct() {
        let size = FileSize::new(500);
        assert_eq!(size.as_u64(), 500);
    }

    #[test]
    fn file_size_is_zero() {
        let size = FileSize::new(0);
        assert!(size.is_zero());
        let size = FileSize::new(1);
        assert!(!size.is_zero());
    }

    #[test]
    fn file_size_display_zero() {
        let size = FileSize::new(0);
        assert_eq!(format!("{size}"), "0 B");
    }

    #[test]
    fn file_size_display_bytes() {
        let size = FileSize::new(500);
        assert_eq!(format!("{size}"), "500 B");
    }

    #[test]
    fn file_size_display_kb() {
        // 1.5 KB = 1536 bytes
        let size = FileSize::new(1536);
        assert_eq!(format!("{size}"), "1.5 KB");
    }

    #[test]
    fn file_size_display_mb() {
        // 2.3 MB = 2.3 * 1024 * 1024 ≈ 2411725
        let size = FileSize::new(2411725);
        assert_eq!(format!("{size}"), "2.3 MB");
    }

    #[test]
    fn file_size_display_gb() {
        // 1.1 GB = 1.1 * 1024^3 ≈ 1181116006
        let size = FileSize::new(1181116006);
        assert_eq!(format!("{size}"), "1.1 GB");
    }

    #[test]
    fn file_size_serde_roundtrip() -> Result<()> {
        let size = FileSize::new(12345);
        let json =
            serde_json::to_string(&size).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        let deserialized: FileSize =
            serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        assert_eq!(size, deserialized);
        Ok(())
    }

    // -- FileExtension ------------------------------------------------------

    #[test]
    fn file_extension_valid() -> Result<()> {
        let ext = FileExtension::new("jpg")?;
        assert_eq!(ext.as_str(), "jpg");
        Ok(())
    }

    #[test]
    fn file_extension_empty() {
        let err = FileExtension::new("").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn file_extension_leading_dot() {
        let err = FileExtension::new(".jpg").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn file_extension_uppercase() {
        let err = FileExtension::new("JPG").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn file_extension_non_alphanumeric() {
        let err = FileExtension::new("jp+g").unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn file_extension_serde_roundtrip() -> Result<()> {
        let ext = FileExtension::new("jpg")?;
        let json =
            serde_json::to_string(&ext).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        let deserialized: FileExtension =
            serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
        assert_eq!(ext, deserialized);
        Ok(())
    }

    #[test]
    fn file_extension_display() -> Result<()> {
        let ext = FileExtension::new("jpg")?;
        assert_eq!(format!("{ext}"), "jpg");
        Ok(())
    }

    // -- FileType ------------------------------------------------------------

    #[test]
    fn file_type_display() {
        assert_eq!(format!("{}", FileType::Jpeg), "jpeg");
        assert_eq!(format!("{}", FileType::Png), "png");
        assert_eq!(format!("{}", FileType::Gif), "gif");
        assert_eq!(format!("{}", FileType::WebP), "webp");
        assert_eq!(format!("{}", FileType::Bmp), "bmp");
        assert_eq!(format!("{}", FileType::Svg), "svg");
        assert_eq!(format!("{}", FileType::Tiff), "tiff");
        assert_eq!(format!("{}", FileType::Mp4), "mp4");
        assert_eq!(format!("{}", FileType::Mov), "mov");
        assert_eq!(format!("{}", FileType::Avi), "avi");
        assert_eq!(format!("{}", FileType::Mkv), "mkv");
        assert_eq!(format!("{}", FileType::WebM), "webm");
        assert_eq!(format!("{}", FileType::Mp3), "mp3");
        assert_eq!(format!("{}", FileType::Flac), "flac");
        assert_eq!(format!("{}", FileType::Wav), "wav");
        assert_eq!(format!("{}", FileType::Ogg), "ogg");
        assert_eq!(format!("{}", FileType::Aac), "aac");
        assert_eq!(format!("{}", FileType::M4a), "m4a");
        assert_eq!(format!("{}", FileType::Pdf), "pdf");
        assert_eq!(format!("{}", FileType::Doc), "doc");
        assert_eq!(format!("{}", FileType::Docx), "docx");
        assert_eq!(format!("{}", FileType::Xls), "xls");
        assert_eq!(format!("{}", FileType::Xlsx), "xlsx");
        assert_eq!(format!("{}", FileType::Ppt), "ppt");
        assert_eq!(format!("{}", FileType::Pptx), "pptx");
        assert_eq!(format!("{}", FileType::Zip), "zip");
        assert_eq!(format!("{}", FileType::Rar), "rar");
        assert_eq!(format!("{}", FileType::TarGz), "tar_gz");
        assert_eq!(format!("{}", FileType::SevenZ), "seven_z");
        assert_eq!(format!("{}", FileType::Unknown), "unknown");
    }

    #[test]
    fn file_type_from_str_roundtrip() -> Result<()> {
        assert_eq!("jpeg".parse::<FileType>()?, FileType::Jpeg);
        assert_eq!("png".parse::<FileType>()?, FileType::Png);
        assert_eq!("gif".parse::<FileType>()?, FileType::Gif);
        assert_eq!("webp".parse::<FileType>()?, FileType::WebP);
        assert_eq!("bmp".parse::<FileType>()?, FileType::Bmp);
        assert_eq!("svg".parse::<FileType>()?, FileType::Svg);
        assert_eq!("tiff".parse::<FileType>()?, FileType::Tiff);
        assert_eq!("mp4".parse::<FileType>()?, FileType::Mp4);
        assert_eq!("mov".parse::<FileType>()?, FileType::Mov);
        assert_eq!("avi".parse::<FileType>()?, FileType::Avi);
        assert_eq!("mkv".parse::<FileType>()?, FileType::Mkv);
        assert_eq!("webm".parse::<FileType>()?, FileType::WebM);
        assert_eq!("mp3".parse::<FileType>()?, FileType::Mp3);
        assert_eq!("flac".parse::<FileType>()?, FileType::Flac);
        assert_eq!("wav".parse::<FileType>()?, FileType::Wav);
        assert_eq!("ogg".parse::<FileType>()?, FileType::Ogg);
        assert_eq!("aac".parse::<FileType>()?, FileType::Aac);
        assert_eq!("m4a".parse::<FileType>()?, FileType::M4a);
        assert_eq!("pdf".parse::<FileType>()?, FileType::Pdf);
        assert_eq!("doc".parse::<FileType>()?, FileType::Doc);
        assert_eq!("docx".parse::<FileType>()?, FileType::Docx);
        assert_eq!("xls".parse::<FileType>()?, FileType::Xls);
        assert_eq!("xlsx".parse::<FileType>()?, FileType::Xlsx);
        assert_eq!("ppt".parse::<FileType>()?, FileType::Ppt);
        assert_eq!("pptx".parse::<FileType>()?, FileType::Pptx);
        assert_eq!("zip".parse::<FileType>()?, FileType::Zip);
        assert_eq!("rar".parse::<FileType>()?, FileType::Rar);
        assert_eq!("tar_gz".parse::<FileType>()?, FileType::TarGz);
        assert_eq!("seven_z".parse::<FileType>()?, FileType::SevenZ);
        Ok(())
    }

    #[test]
    fn file_type_from_str_case_insensitive() -> Result<()> {
        assert_eq!("JPEG".parse::<FileType>()?, FileType::Jpeg);
        assert_eq!("Tar_Gz".parse::<FileType>()?, FileType::TarGz);
        Ok(())
    }

    #[test]
    fn file_type_unknown_for_unrecognized() -> Result<()> {
        assert_eq!("unknown".parse::<FileType>()?, FileType::Unknown);
        assert_eq!("foobar".parse::<FileType>()?, FileType::Unknown);
        assert_eq!("".parse::<FileType>()?, FileType::Unknown);
        Ok(())
    }

    #[test]
    fn file_type_category_image() {
        assert_eq!(FileType::Jpeg.category(), FileCategory::Image);
        assert_eq!(FileType::Png.category(), FileCategory::Image);
        assert_eq!(FileType::Gif.category(), FileCategory::Image);
        assert_eq!(FileType::WebP.category(), FileCategory::Image);
        assert_eq!(FileType::Bmp.category(), FileCategory::Image);
        assert_eq!(FileType::Svg.category(), FileCategory::Image);
        assert_eq!(FileType::Tiff.category(), FileCategory::Image);
    }

    #[test]
    fn file_type_category_video() {
        assert_eq!(FileType::Mp4.category(), FileCategory::Video);
        assert_eq!(FileType::Mov.category(), FileCategory::Video);
        assert_eq!(FileType::Avi.category(), FileCategory::Video);
        assert_eq!(FileType::Mkv.category(), FileCategory::Video);
        assert_eq!(FileType::WebM.category(), FileCategory::Video);
    }

    #[test]
    fn file_type_category_audio() {
        assert_eq!(FileType::Mp3.category(), FileCategory::Audio);
        assert_eq!(FileType::Flac.category(), FileCategory::Audio);
        assert_eq!(FileType::Wav.category(), FileCategory::Audio);
        assert_eq!(FileType::Ogg.category(), FileCategory::Audio);
        assert_eq!(FileType::Aac.category(), FileCategory::Audio);
        assert_eq!(FileType::M4a.category(), FileCategory::Audio);
    }

    #[test]
    fn file_type_category_document() {
        assert_eq!(FileType::Pdf.category(), FileCategory::Document);
        assert_eq!(FileType::Doc.category(), FileCategory::Document);
        assert_eq!(FileType::Docx.category(), FileCategory::Document);
        assert_eq!(FileType::Xls.category(), FileCategory::Document);
        assert_eq!(FileType::Xlsx.category(), FileCategory::Document);
        assert_eq!(FileType::Ppt.category(), FileCategory::Document);
        assert_eq!(FileType::Pptx.category(), FileCategory::Document);
    }

    #[test]
    fn file_type_category_archive() {
        assert_eq!(FileType::Zip.category(), FileCategory::Archive);
        assert_eq!(FileType::Rar.category(), FileCategory::Archive);
        assert_eq!(FileType::TarGz.category(), FileCategory::Archive);
        assert_eq!(FileType::SevenZ.category(), FileCategory::Archive);
    }

    #[test]
    fn file_type_category_unknown() {
        assert_eq!(FileType::Unknown.category(), FileCategory::Other);
    }

    #[test]
    fn file_type_serde_roundtrip() -> Result<()> {
        let variants = [
            FileType::Jpeg,
            FileType::TarGz,
            FileType::SevenZ,
            FileType::Unknown,
        ];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: FileType =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }

    // -- FileCategory --------------------------------------------------------

    #[test]
    fn file_category_display() {
        assert_eq!(format!("{}", FileCategory::Image), "image");
        assert_eq!(format!("{}", FileCategory::Video), "video");
        assert_eq!(format!("{}", FileCategory::Audio), "audio");
        assert_eq!(format!("{}", FileCategory::Document), "document");
        assert_eq!(format!("{}", FileCategory::Archive), "archive");
        assert_eq!(format!("{}", FileCategory::Other), "other");
    }

    #[test]
    fn file_category_from_str_roundtrip() -> Result<()> {
        assert_eq!("image".parse::<FileCategory>()?, FileCategory::Image);
        assert_eq!("video".parse::<FileCategory>()?, FileCategory::Video);
        assert_eq!("audio".parse::<FileCategory>()?, FileCategory::Audio);
        assert_eq!("document".parse::<FileCategory>()?, FileCategory::Document);
        assert_eq!("archive".parse::<FileCategory>()?, FileCategory::Archive);
        assert_eq!("other".parse::<FileCategory>()?, FileCategory::Other);
        Ok(())
    }

    #[test]
    fn file_category_from_str_invalid() {
        let err = "bogus".parse::<FileCategory>().unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn file_category_serde_roundtrip() -> Result<()> {
        let variants = [
            FileCategory::Image,
            FileCategory::Video,
            FileCategory::Audio,
            FileCategory::Document,
            FileCategory::Archive,
            FileCategory::Other,
        ];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: FileCategory =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }

    // -- MetadataNamespace ---------------------------------------------------

    #[test]
    fn metadata_namespace_display_standard() {
        assert_eq!(format!("{}", MetadataNamespace::Exif), "exif");
        assert_eq!(format!("{}", MetadataNamespace::Video), "video");
        assert_eq!(format!("{}", MetadataNamespace::Audio), "audio");
        assert_eq!(format!("{}", MetadataNamespace::General), "general");
    }

    #[test]
    fn metadata_namespace_display_custom() {
        let ns = MetadataNamespace::Custom("XMP".to_string());
        assert_eq!(format!("{ns}"), "XMP");
    }

    #[test]
    fn metadata_namespace_from_str_roundtrip() -> Result<()> {
        assert_eq!(
            "exif".parse::<MetadataNamespace>()?,
            MetadataNamespace::Exif
        );
        assert_eq!(
            "video".parse::<MetadataNamespace>()?,
            MetadataNamespace::Video
        );
        assert_eq!(
            "audio".parse::<MetadataNamespace>()?,
            MetadataNamespace::Audio
        );
        assert_eq!(
            "general".parse::<MetadataNamespace>()?,
            MetadataNamespace::General
        );
        Ok(())
    }

    #[test]
    fn metadata_namespace_from_str_invalid() {
        let err = "custom".parse::<MetadataNamespace>().unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn metadata_namespace_serde_roundtrip() -> Result<()> {
        let variants = [
            MetadataNamespace::Exif,
            MetadataNamespace::Video,
            MetadataNamespace::Audio,
            MetadataNamespace::General,
            MetadataNamespace::Custom("XMP".to_string()),
        ];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: MetadataNamespace =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }

    // -- MetadataState -------------------------------------------------------

    #[test]
    fn metadata_state_display() {
        assert_eq!(format!("{}", MetadataState::Pending), "pending");
        assert_eq!(format!("{}", MetadataState::Extracted), "extracted");
        assert_eq!(format!("{}", MetadataState::Failed), "failed");
    }

    #[test]
    fn metadata_state_from_str_roundtrip() -> Result<()> {
        assert_eq!("pending".parse::<MetadataState>()?, MetadataState::Pending);
        assert_eq!(
            "extracted".parse::<MetadataState>()?,
            MetadataState::Extracted
        );
        assert_eq!("failed".parse::<MetadataState>()?, MetadataState::Failed);
        Ok(())
    }

    #[test]
    fn metadata_state_from_str_invalid() {
        let err = "bogus".parse::<MetadataState>().unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn metadata_state_serde_roundtrip() -> Result<()> {
        let variants = [
            MetadataState::Pending,
            MetadataState::Extracted,
            MetadataState::Failed,
        ];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: MetadataState =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }

    // -- SyncStatus ----------------------------------------------------------

    #[test]
    fn sync_status_display() {
        assert_eq!(format!("{}", SyncStatus::Synced), "synced");
        assert_eq!(format!("{}", SyncStatus::Modified), "modified");
        assert_eq!(format!("{}", SyncStatus::New), "new");
        assert_eq!(format!("{}", SyncStatus::Deleted), "deleted");
    }

    #[test]
    fn sync_status_from_str_roundtrip() -> Result<()> {
        assert_eq!("synced".parse::<SyncStatus>()?, SyncStatus::Synced);
        assert_eq!("modified".parse::<SyncStatus>()?, SyncStatus::Modified);
        assert_eq!("new".parse::<SyncStatus>()?, SyncStatus::New);
        assert_eq!("deleted".parse::<SyncStatus>()?, SyncStatus::Deleted);
        Ok(())
    }

    #[test]
    fn sync_status_from_str_invalid() {
        let err = "bogus".parse::<SyncStatus>().unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn sync_status_serde_roundtrip() -> Result<()> {
        let variants = [
            SyncStatus::Synced,
            SyncStatus::Modified,
            SyncStatus::New,
            SyncStatus::Deleted,
        ];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: SyncStatus =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }

    // -- DbStatus ------------------------------------------------------------

    #[test]
    fn db_status_display() {
        assert_eq!(format!("{}", DbStatus::NotCreated), "not_created");
        assert_eq!(format!("{}", DbStatus::Stale), "stale");
        assert_eq!(format!("{}", DbStatus::Fresh), "fresh");
        assert_eq!(format!("{}", DbStatus::Locked), "locked");
        assert_eq!(format!("{}", DbStatus::Corrupted), "corrupted");
    }

    #[test]
    fn db_status_from_str_roundtrip() -> Result<()> {
        assert_eq!("not_created".parse::<DbStatus>()?, DbStatus::NotCreated);
        assert_eq!("stale".parse::<DbStatus>()?, DbStatus::Stale);
        assert_eq!("fresh".parse::<DbStatus>()?, DbStatus::Fresh);
        assert_eq!("locked".parse::<DbStatus>()?, DbStatus::Locked);
        assert_eq!("corrupted".parse::<DbStatus>()?, DbStatus::Corrupted);
        Ok(())
    }

    #[test]
    fn db_status_from_str_invalid() {
        let err = "bogus".parse::<DbStatus>().unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn db_status_serde_roundtrip() -> Result<()> {
        let variants = [
            DbStatus::NotCreated,
            DbStatus::Stale,
            DbStatus::Fresh,
            DbStatus::Locked,
            DbStatus::Corrupted,
        ];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: DbStatus =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }

    // -- DbAction ------------------------------------------------------------

    #[test]
    fn db_action_display() {
        assert_eq!(format!("{}", DbAction::Create), "create");
        assert_eq!(format!("{}", DbAction::Download), "download");
        assert_eq!(format!("{}", DbAction::Reuse), "reuse");
        assert_eq!(format!("{}", DbAction::Recover), "recover");
        assert_eq!(format!("{}", DbAction::Wait), "wait");
    }

    #[test]
    fn db_action_from_str_roundtrip() -> Result<()> {
        assert_eq!("create".parse::<DbAction>()?, DbAction::Create);
        assert_eq!("download".parse::<DbAction>()?, DbAction::Download);
        assert_eq!("reuse".parse::<DbAction>()?, DbAction::Reuse);
        assert_eq!("recover".parse::<DbAction>()?, DbAction::Recover);
        assert_eq!("wait".parse::<DbAction>()?, DbAction::Wait);
        Ok(())
    }

    #[test]
    fn db_action_from_str_invalid() {
        let err = "bogus".parse::<DbAction>().unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn db_action_serde_roundtrip() -> Result<()> {
        let variants = [
            DbAction::Create,
            DbAction::Download,
            DbAction::Reuse,
            DbAction::Recover,
            DbAction::Wait,
        ];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: DbAction =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }

    // -- SortField -----------------------------------------------------------

    #[test]
    fn sort_field_display() {
        assert_eq!(format!("{}", SortField::Name), "name");
        assert_eq!(format!("{}", SortField::Size), "size");
        assert_eq!(format!("{}", SortField::LastModified), "last_modified");
        assert_eq!(format!("{}", SortField::FileType), "file_type");
    }

    #[test]
    fn sort_field_from_str_roundtrip() -> Result<()> {
        assert_eq!("name".parse::<SortField>()?, SortField::Name);
        assert_eq!("size".parse::<SortField>()?, SortField::Size);
        assert_eq!(
            "last_modified".parse::<SortField>()?,
            SortField::LastModified
        );
        assert_eq!("file_type".parse::<SortField>()?, SortField::FileType);
        Ok(())
    }

    #[test]
    fn sort_field_from_str_invalid() {
        let err = "bogus".parse::<SortField>().unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn sort_field_serde_roundtrip() -> Result<()> {
        let variants = [
            SortField::Name,
            SortField::Size,
            SortField::LastModified,
            SortField::FileType,
        ];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: SortField =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }

    // -- SortOrder -----------------------------------------------------------

    #[test]
    fn sort_order_display() {
        assert_eq!(format!("{}", SortOrder::Ascending), "ascending");
        assert_eq!(format!("{}", SortOrder::Descending), "descending");
    }

    #[test]
    fn sort_order_from_str_roundtrip() -> Result<()> {
        assert_eq!("ascending".parse::<SortOrder>()?, SortOrder::Ascending);
        assert_eq!("descending".parse::<SortOrder>()?, SortOrder::Descending);
        Ok(())
    }

    #[test]
    fn sort_order_from_str_invalid() {
        let err = "bogus".parse::<SortOrder>().unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn sort_order_serde_roundtrip() -> Result<()> {
        let variants = [SortOrder::Ascending, SortOrder::Descending];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: SortOrder =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }

    // -- ViewMode ------------------------------------------------------------

    #[test]
    fn view_mode_display() {
        assert_eq!(format!("{}", ViewMode::List), "list");
        assert_eq!(format!("{}", ViewMode::Grid), "grid");
        assert_eq!(format!("{}", ViewMode::Timeline), "timeline");
    }

    #[test]
    fn view_mode_from_str_roundtrip() -> Result<()> {
        assert_eq!("list".parse::<ViewMode>()?, ViewMode::List);
        assert_eq!("grid".parse::<ViewMode>()?, ViewMode::Grid);
        assert_eq!("timeline".parse::<ViewMode>()?, ViewMode::Timeline);
        Ok(())
    }

    #[test]
    fn view_mode_from_str_invalid() {
        let err = "bogus".parse::<ViewMode>().unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn view_mode_serde_roundtrip() -> Result<()> {
        let variants = [ViewMode::List, ViewMode::Grid, ViewMode::Timeline];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: ViewMode =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }

    // -- ScanMode ------------------------------------------------------------

    #[test]
    fn scan_mode_display() {
        assert_eq!(format!("{}", ScanMode::Full), "full");
        assert_eq!(format!("{}", ScanMode::Incremental), "incremental");
    }

    #[test]
    fn scan_mode_from_str_roundtrip() -> Result<()> {
        assert_eq!("full".parse::<ScanMode>()?, ScanMode::Full);
        assert_eq!("incremental".parse::<ScanMode>()?, ScanMode::Incremental);
        Ok(())
    }

    #[test]
    fn scan_mode_from_str_invalid() {
        let err = "bogus".parse::<ScanMode>().unwrap_err();
        assert!(matches!(err, S3GalleryError::ValidationError(_)));
    }

    #[test]
    fn scan_mode_serde_roundtrip() -> Result<()> {
        let variants = [ScanMode::Full, ScanMode::Incremental];
        for v in &variants {
            let json =
                serde_json::to_string(v).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            let deserialized: ScanMode =
                serde_json::from_str(&json).map_err(|e| S3GalleryError::Internal(e.to_string()))?;
            assert_eq!(*v, deserialized);
        }
        Ok(())
    }
}
