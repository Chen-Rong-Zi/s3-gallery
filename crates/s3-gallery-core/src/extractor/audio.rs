use async_trait::async_trait;

use super::registry::{MetadataExtractor, MetadataItem};
use crate::error::Result;

/// Audio metadata extractor (stub).
pub struct AudioExtractor;

impl AudioExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AudioExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MetadataExtractor for AudioExtractor {
    fn name(&self) -> &'static str {
        "audio"
    }

    fn supports(&self, _file_type: &str, extension: &str) -> bool {
        matches!(extension, "mp3" | "flac" | "wav" | "ogg" | "aac" | "m4a")
    }

    /// Extract metadata from audio files.
    ///
    /// # Errors
    ///
    /// Currently a stub that always succeeds with an empty result.
    async fn extract(&self, _data: &[u8], _extension: &str) -> Result<Vec<MetadataItem>> {
        // Stub: return empty metadata for now.
        // TODO: Implement actual audio metadata extraction
        Ok(Vec::new())
    }
}
