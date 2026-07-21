use async_trait::async_trait;

use crate::error::{OssgalleyError, Result};
use super::registry::{MetadataExtractor, MetadataItem};

/// EXIF metadata extractor for JPEG/TIFF images.
pub struct ExifExtractor;

impl ExifExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExifExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MetadataExtractor for ExifExtractor {
    fn name(&self) -> &'static str {
        "exif"
    }

    fn supports(&self, _file_type: &str, extension: &str) -> bool {
        matches!(extension, "jpg" | "jpeg" | "tiff" | "tif" | "heic" | "heif")
    }

    /// Extract EXIF metadata from image data.
    ///
    /// # Errors
    ///
    /// Returns `OssgalleyError::MetadataExtraction` if the EXIF data cannot be
    /// parsed.
    async fn extract(&self, data: &[u8], _extension: &str) -> Result<Vec<MetadataItem>> {
        let reader = exif::Reader::new();
        let exif_data = reader
            .read_from_container(&mut std::io::BufReader::new(std::io::Cursor::new(data)))
            .map_err(|e| {
                OssgalleyError::MetadataExtraction(format!("EXIF parse error: {e}"))
            })?;

        let mut items = Vec::new();
        for field in exif_data.fields() {
            // Skip binary fields that are too large.
            let value_str = field.display_value().to_string();
            if value_str.len() > 1024 {
                continue;
            }

            items.push(MetadataItem {
                namespace: "exif",
                key: field.tag.to_string(),
                value: value_str,
            });
        }

        Ok(items)
    }
}