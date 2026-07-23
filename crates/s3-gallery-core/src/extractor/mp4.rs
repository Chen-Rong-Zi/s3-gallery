use async_trait::async_trait;

use super::registry::{MetadataExtractor, MetadataItem};
use crate::error::Result;

/// MP4 metadata extractor (stub).
pub struct Mp4Extractor;

impl Mp4Extractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Mp4Extractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MetadataExtractor for Mp4Extractor {
    fn name(&self) -> &'static str {
        "mp4"
    }

    fn supports(&self, _file_type: &str, extension: &str) -> bool {
        matches!(extension, "mp4" | "mov" | "m4v" | "m4a")
    }

    /// Extract metadata from MP4 files.
    ///
    /// # Errors
    ///
    /// Currently a stub that always succeeds with an empty result.
    async fn extract(&self, _data: &[u8], _extension: &str) -> Result<Vec<MetadataItem>> {
        // Stub: return empty metadata for now.
        // TODO: Implement actual MP4 metadata extraction
        Ok(Vec::new())
    }
}
