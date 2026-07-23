use async_trait::async_trait;

use crate::error::Result;

/// A single metadata item extracted from a file.
#[derive(Debug, Clone)]
pub struct MetadataItem {
    pub namespace: &'static str,
    pub key: String,
    pub value: String,
}

/// Trait for metadata extractors.
#[async_trait]
pub trait MetadataExtractor: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports(&self, file_type: &str, extension: &str) -> bool;
    async fn extract(&self, data: &[u8], extension: &str) -> Result<Vec<MetadataItem>>;
}

/// Registry of metadata extractors.
pub struct ExtractorRegistry {
    extractors: Vec<Box<dyn MetadataExtractor>>,
}

impl ExtractorRegistry {
    pub fn new() -> Self {
        Self {
            extractors: Vec::new(),
        }
    }

    pub fn register(&mut self, extractor: Box<dyn MetadataExtractor>) {
        self.extractors.push(extractor);
    }

    /// Find all matching extractors for a given file type and extension.
    pub fn find(&self, file_type: &str, extension: &str) -> Vec<&dyn MetadataExtractor> {
        self.extractors
            .iter()
            .filter(|e| e.supports(file_type, extension))
            .map(|e| e.as_ref())
            .collect()
    }

    /// Find the first matching extractor.
    pub fn find_first(&self, file_type: &str, extension: &str) -> Option<&dyn MetadataExtractor> {
        self.extractors
            .iter()
            .find(|e| e.supports(file_type, extension))
            .map(|e| e.as_ref())
    }

    /// Extract metadata using all matching extractors.
    ///
    /// # Errors
    ///
    /// Returns `S3GalleryError::MetadataExtraction` if any extractor fails to
    /// parse the provided data.
    pub async fn extract_all(
        &self,
        data: &[u8],
        file_type: &str,
        extension: &str,
    ) -> Result<Vec<MetadataItem>> {
        let mut all_items = Vec::new();
        for extractor in self.find(file_type, extension) {
            let items = extractor.extract(data, extension).await?;
            all_items.extend(items);
        }
        Ok(all_items)
    }
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_empty() {
        let registry = ExtractorRegistry::new();
        assert!(registry.find("jpeg", "jpg").is_empty());
    }

    #[test]
    fn test_registry_register_and_find() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(crate::extractor::exif::ExifExtractor::new()));

        let extractors = registry.find("jpeg", "jpg");
        assert_eq!(extractors.len(), 1);
        if let Some(e) = extractors.first() {
            assert_eq!(e.name(), "exif");
        }
    }

    #[test]
    fn test_registry_find_no_match() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(crate::extractor::exif::ExifExtractor::new()));

        let extractors = registry.find("unknown", "xyz");
        assert!(extractors.is_empty());
    }

    #[test]
    fn test_registry_find_first() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(crate::extractor::exif::ExifExtractor::new()));

        let extractor = registry.find_first("jpeg", "jpg");
        assert!(extractor.is_some());
        if let Some(e) = extractor {
            assert_eq!(e.name(), "exif");
        }
    }

    #[test]
    fn test_registry_find_first_none() {
        let registry = ExtractorRegistry::new();
        let extractor = registry.find_first("jpeg", "jpg");
        assert!(extractor.is_none());
    }

    #[test]
    fn test_registry_multiple_extractors() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(crate::extractor::exif::ExifExtractor::new()));
        registry.register(Box::new(crate::extractor::mp4::Mp4Extractor::new()));

        // jpg should only match exif
        let jpg_extractors = registry.find("jpeg", "jpg");
        assert_eq!(jpg_extractors.len(), 1);
        if let Some(e) = jpg_extractors.first() {
            assert_eq!(e.name(), "exif");
        }

        // mp4 should only match mp4 extractor
        let mp4_extractors = registry.find("mp4", "mp4");
        assert_eq!(mp4_extractors.len(), 1);
        if let Some(e) = mp4_extractors.first() {
            assert_eq!(e.name(), "mp4");
        }
    }
}
