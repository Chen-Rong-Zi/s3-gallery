use async_trait::async_trait;

use super::registry::{MetadataExtractor, MetadataItem};
use crate::error::{Result, S3GalleryError};

/// EXIF metadata extractor for JPEG/TIFF images.
pub struct ExifExtractor;

impl ExifExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Extract EXIF by manually scanning the JPEG APP1 marker,
    /// then parsing the raw TIFF data with `read_raw()`.
    /// This avoids the kamadak-exif JPEG parser which reads all segments
    /// and fails on truncated data.
    fn extract_manual(&self, data: &[u8]) -> Result<Vec<MetadataItem>> {
        // Find the APP1 marker (0xFF 0xE1) and extract raw EXIF data
        let exif_tiff = Self::find_exif_in_jpeg(data).ok_or_else(|| {
            S3GalleryError::MetadataExtraction("No Exif data found in JPEG".to_string())
        })?;

        // Parse the raw TIFF EXIF data
        let reader = exif::Reader::new();
        let exif_data = reader
            .read_raw(exif_tiff)
            .map_err(|e| S3GalleryError::MetadataExtraction(format!("EXIF parse error: {e}")))?;

        let mut items = Vec::new();
        for field in exif_data.fields() {
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

    /// Scan JPEG data for the APP1 marker (0xFF 0xE1) and extract the
    /// raw EXIF TIFF data. Returns None if APP1/EXIF is not found.
    ///
    /// This is a simple byte scan, not a full JPEG parser — it only
    /// looks for the EXIF marker and ignores all other segments.
    fn find_exif_in_jpeg(data: &[u8]) -> Option<Vec<u8>> {
        // Check JPEG SOI (0xFF 0xD8)
        if data.first().copied() != Some(0xFF) || data.get(1).copied() != Some(0xD8) {
            return None;
        }

        let mut pos: usize = 2;
        while pos + 4 <= data.len() {
            // Skip non-0xFF bytes (scan data or padding)
            if data.get(pos).copied() != Some(0xFF) {
                pos += 1;
                continue;
            }

            let marker = data.get(pos + 1).copied()?;

            // Skip stand-alone markers (no segment data)
            if marker == 0x00 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
                pos += 2;
                continue;
            }

            // EOI (End of Image) — no more markers
            if marker == 0xD9 {
                break;
            }

            // SOS (Start of Scan) — compressed data follows, stop searching
            if marker == 0xDA {
                break;
            }

            // Read segment length (2 bytes, big-endian)
            if pos + 4 > data.len() {
                break;
            }
            let b0 = data.get(pos + 2).copied().unwrap_or(0);
            let b1 = data.get(pos + 3).copied().unwrap_or(0);
            let seg_len = u16::from_be_bytes([b0, b1]) as usize;
            if seg_len < 2 {
                break;
            }

            let seg_data_start = pos + 4;
            let seg_data_end = pos + 2 + seg_len;

            // Check if this is APP1 (0xE1) with Exif identifier
            if marker == 0xE1
                && seg_data_start + 6 <= data.len()
                && data.get(seg_data_start..seg_data_start + 6) == Some(b"Exif\0\0")
            {
                // Extract the TIFF data (after "Exif\0\0")
                let tiff_start = seg_data_start + 6;
                let tiff_end = seg_data_end.min(data.len());
                if tiff_end > tiff_start {
                    return data.get(tiff_start..tiff_end).map(|s| s.to_vec());
                }
                return None;
            }

            // Move to next segment
            if seg_data_end > data.len() {
                break;
            }
            pos = seg_data_end;
        }

        None
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
    /// Uses a two-layer approach:
    /// 1. Try the standard kamadak-exif JPEG parser
    /// 2. On failure, fall back to manual APP1 scan + read_raw
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::MetadataExtraction` if the EXIF data cannot be
    /// parsed.
    async fn extract(&self, data: &[u8], _extension: &str) -> Result<Vec<MetadataItem>> {
        self.extract_manual(data)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// Verify find_exif_in_jpeg works with a minimal JPEG containing EXIF.
    #[test]
    fn test_find_exif_in_jpeg_valid() {
        // Build a minimal JPEG: SOI + APP1(Exif) + EOI
        // TIFF data: little-endian ("II"), 8-byte header, empty IFD0
        let tiff: Vec<u8> = vec![
            0x49, 0x49, 0x2A, 0x00, // TIFF header (little-endian)
            0x08, 0x00, 0x00, 0x00, // offset to IFD0 = 8
            0x00, 0x00, // entry count = 0 (no entries)
            0x00, 0x00, 0x00, 0x00, // next IFD offset = 0
        ];
        let seg_len: u16 = (6 + tiff.len()) as u16 + 2; // +2 for length field itself
        let mut jpeg = Vec::new();
        jpeg.extend_from_slice(&[0xFF, 0xD8]); // SOI
        jpeg.extend_from_slice(&[0xFF, 0xE1]); // APP1
        jpeg.extend_from_slice(&seg_len.to_be_bytes()); // segment length
        jpeg.extend_from_slice(b"Exif\0\0"); // Exif identifier
        jpeg.extend_from_slice(&tiff); // TIFF data
        jpeg.extend_from_slice(&[0xFF, 0xD9]); // EOI

        let result = ExifExtractor::find_exif_in_jpeg(&jpeg);
        assert!(result.is_some(), "should find EXIF data");
        assert_eq!(result.unwrap(), tiff, "TIFF data should match");
    }

    /// Verify find_exif_in_jpeg returns None for JPEG with no EXIF.
    #[test]
    fn test_find_exif_in_jpeg_no_exif() {
        let jpeg = vec![0xFF, 0xD8, 0xFF, 0xD9]; // SOI + EOI, no APP1
        assert!(ExifExtractor::find_exif_in_jpeg(&jpeg).is_none());
    }

    /// Verify find_exif_in_jpeg returns None for non-JPEG data.
    #[test]
    fn test_find_exif_in_jpeg_not_jpeg() {
        let data = b"not a jpeg file";
        assert!(ExifExtractor::find_exif_in_jpeg(data).is_none());
    }

    /// Verify find_exif_in_jpeg stops at SOS (start of scan).
    #[test]
    fn test_find_exif_in_jpeg_stops_at_sos() {
        let jpeg = vec![
            0xFF, 0xD8, // SOI
            0xFF, 0xDA, // SOS — compressed data follows
            0x00, 0x08, // segment length
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, // "compressed" data
        ];
        assert!(
            ExifExtractor::find_exif_in_jpeg(&jpeg).is_none(),
            "should stop at SOS without EXIF"
        );
    }
}
