use crate::types::{FileExtension, FileType};

/// Built-in extension-to-`FileType` mapping.
///
/// Returns `FileType::Unknown` for any extension not in the built-in table.
pub fn classify_extension(ext: &FileExtension) -> FileType {
    match ext.as_str() {
        // Image
        "jpg" | "jpeg" => FileType::Jpeg,
        "png" => FileType::Png,
        "gif" => FileType::Gif,
        "webp" => FileType::WebP,
        "bmp" => FileType::Bmp,
        "svg" => FileType::Svg,
        "tiff" | "tif" => FileType::Tiff,
        "heic" | "heif" => FileType::Jpeg,

        // Video
        "mp4" => FileType::Mp4,
        "mov" => FileType::Mov,
        "avi" => FileType::Avi,
        "mkv" => FileType::Mkv,
        "webm" => FileType::WebM,
        "m4v" => FileType::Mp4,

        // Audio
        "mp3" => FileType::Mp3,
        "flac" => FileType::Flac,
        "wav" => FileType::Wav,
        "ogg" => FileType::Ogg,
        "aac" => FileType::Aac,
        "m4a" => FileType::M4a,

        // Document
        "pdf" => FileType::Pdf,
        "doc" => FileType::Doc,
        "docx" => FileType::Docx,
        "xls" => FileType::Xls,
        "xlsx" => FileType::Xlsx,
        "ppt" => FileType::Ppt,
        "pptx" => FileType::Pptx,

        // Archive
        "zip" => FileType::Zip,
        "rar" => FileType::Rar,
        "tar" => FileType::TarGz,
        "gz" => FileType::TarGz,
        "7z" => FileType::SevenZ,
        "tgz" => FileType::TarGz,

        _ => FileType::Unknown,
    }
}

/// Derive the MIME content type from a file extension.
///
/// Returns `None` for unrecognized extensions.
pub fn content_type_from_extension(ext: &FileExtension) -> Option<String> {
    let ct = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "tiff" | "tif" => "image/tiff",
        "heic" | "heif" => "image/heic",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "aac" => "audio/aac",
        "m4a" => "audio/mp4",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "rar" => "application/vnd.rar",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "7z" => "application/x-7z-compressed",
        _ => return None,
    };
    Some(ct.to_string())
}

/// Parse a file extension from a filename.
///
/// Returns the portion after the last `.` character, normalized to lowercase.
/// Double extensions (e.g. `.tar.gz`) are handled by returning only the final
/// component (e.g. `"gz"`).
///
/// # Examples
///
/// - `"photo.jpg"` returns `Some("jpg")`
/// - `"archive.tar.gz"` returns `Some("gz")`
/// - `"README"` returns `None`
/// - `""` returns `None`
pub fn parse_extension(filename: &str) -> Option<FileExtension> {
    let filename = filename.trim();
    if filename.is_empty() || filename == "." || filename == ".." {
        return None;
    }

    // Find the last dot
    let dot_idx = filename.rfind('.')?;
    let ext_str = filename.get(dot_idx + 1..)?;

    if ext_str.is_empty() {
        return None;
    }

    // Normalize to lowercase and attempt to create a FileExtension.
    // FileExtension::new validates that the string is non-empty, lowercase
    // alphanumeric only, and has no leading dot — all of which hold here.
    FileExtension::new(ext_str.to_lowercase()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::S3GalleryError;

    // ---------------------------------------------------------------------------
    // classify_extension
    // ---------------------------------------------------------------------------

    #[test]
    fn classify_image_extensions() -> Result<(), S3GalleryError> {
        assert_eq!(
            classify_extension(&FileExtension::new("jpg")?),
            FileType::Jpeg
        );
        assert_eq!(
            classify_extension(&FileExtension::new("jpeg")?),
            FileType::Jpeg
        );
        assert_eq!(
            classify_extension(&FileExtension::new("png")?),
            FileType::Png
        );
        assert_eq!(
            classify_extension(&FileExtension::new("gif")?),
            FileType::Gif
        );
        assert_eq!(
            classify_extension(&FileExtension::new("webp")?),
            FileType::WebP
        );
        assert_eq!(
            classify_extension(&FileExtension::new("bmp")?),
            FileType::Bmp
        );
        assert_eq!(
            classify_extension(&FileExtension::new("svg")?),
            FileType::Svg
        );
        assert_eq!(
            classify_extension(&FileExtension::new("tiff")?),
            FileType::Tiff
        );
        assert_eq!(
            classify_extension(&FileExtension::new("tif")?),
            FileType::Tiff
        );
        assert_eq!(
            classify_extension(&FileExtension::new("heic")?),
            FileType::Jpeg
        );
        assert_eq!(
            classify_extension(&FileExtension::new("heif")?),
            FileType::Jpeg
        );
        Ok(())
    }

    #[test]
    fn classify_video_extensions() -> Result<(), S3GalleryError> {
        assert_eq!(
            classify_extension(&FileExtension::new("mp4")?),
            FileType::Mp4
        );
        assert_eq!(
            classify_extension(&FileExtension::new("mov")?),
            FileType::Mov
        );
        assert_eq!(
            classify_extension(&FileExtension::new("avi")?),
            FileType::Avi
        );
        assert_eq!(
            classify_extension(&FileExtension::new("mkv")?),
            FileType::Mkv
        );
        assert_eq!(
            classify_extension(&FileExtension::new("webm")?),
            FileType::WebM
        );
        assert_eq!(
            classify_extension(&FileExtension::new("m4v")?),
            FileType::Mp4
        );
        Ok(())
    }

    #[test]
    fn classify_audio_extensions() -> Result<(), S3GalleryError> {
        assert_eq!(
            classify_extension(&FileExtension::new("mp3")?),
            FileType::Mp3
        );
        assert_eq!(
            classify_extension(&FileExtension::new("flac")?),
            FileType::Flac
        );
        assert_eq!(
            classify_extension(&FileExtension::new("wav")?),
            FileType::Wav
        );
        assert_eq!(
            classify_extension(&FileExtension::new("ogg")?),
            FileType::Ogg
        );
        assert_eq!(
            classify_extension(&FileExtension::new("aac")?),
            FileType::Aac
        );
        assert_eq!(
            classify_extension(&FileExtension::new("m4a")?),
            FileType::M4a
        );
        Ok(())
    }

    #[test]
    fn classify_document_extensions() -> Result<(), S3GalleryError> {
        assert_eq!(
            classify_extension(&FileExtension::new("pdf")?),
            FileType::Pdf
        );
        assert_eq!(
            classify_extension(&FileExtension::new("doc")?),
            FileType::Doc
        );
        assert_eq!(
            classify_extension(&FileExtension::new("docx")?),
            FileType::Docx
        );
        assert_eq!(
            classify_extension(&FileExtension::new("xls")?),
            FileType::Xls
        );
        assert_eq!(
            classify_extension(&FileExtension::new("xlsx")?),
            FileType::Xlsx
        );
        assert_eq!(
            classify_extension(&FileExtension::new("ppt")?),
            FileType::Ppt
        );
        assert_eq!(
            classify_extension(&FileExtension::new("pptx")?),
            FileType::Pptx
        );
        Ok(())
    }

    #[test]
    fn classify_archive_extensions() -> Result<(), S3GalleryError> {
        assert_eq!(
            classify_extension(&FileExtension::new("zip")?),
            FileType::Zip
        );
        assert_eq!(
            classify_extension(&FileExtension::new("rar")?),
            FileType::Rar
        );
        assert_eq!(
            classify_extension(&FileExtension::new("tar")?),
            FileType::TarGz
        );
        assert_eq!(
            classify_extension(&FileExtension::new("gz")?),
            FileType::TarGz
        );
        assert_eq!(
            classify_extension(&FileExtension::new("7z")?),
            FileType::SevenZ
        );
        assert_eq!(
            classify_extension(&FileExtension::new("tgz")?),
            FileType::TarGz
        );
        Ok(())
    }

    #[test]
    fn classify_unknown_extension() -> Result<(), S3GalleryError> {
        assert_eq!(
            classify_extension(&FileExtension::new("xyz")?),
            FileType::Unknown
        );
        assert_eq!(
            classify_extension(&FileExtension::new("foo")?),
            FileType::Unknown
        );
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // content_type_from_extension
    // ---------------------------------------------------------------------------

    #[test]
    fn content_type_jpeg() -> Result<(), S3GalleryError> {
        assert_eq!(
            content_type_from_extension(&FileExtension::new("jpg")?),
            Some("image/jpeg".to_string())
        );
        assert_eq!(
            content_type_from_extension(&FileExtension::new("jpeg")?),
            Some("image/jpeg".to_string())
        );
        Ok(())
    }

    #[test]
    fn content_type_png() -> Result<(), S3GalleryError> {
        assert_eq!(
            content_type_from_extension(&FileExtension::new("png")?),
            Some("image/png".to_string())
        );
        Ok(())
    }

    #[test]
    fn content_type_video() -> Result<(), S3GalleryError> {
        assert_eq!(
            content_type_from_extension(&FileExtension::new("mp4")?),
            Some("video/mp4".to_string())
        );
        assert_eq!(
            content_type_from_extension(&FileExtension::new("mov")?),
            Some("video/quicktime".to_string())
        );
        Ok(())
    }

    #[test]
    fn content_type_audio() -> Result<(), S3GalleryError> {
        assert_eq!(
            content_type_from_extension(&FileExtension::new("mp3")?),
            Some("audio/mpeg".to_string())
        );
        assert_eq!(
            content_type_from_extension(&FileExtension::new("flac")?),
            Some("audio/flac".to_string())
        );
        Ok(())
    }

    #[test]
    fn content_type_unknown() -> Result<(), S3GalleryError> {
        assert_eq!(
            content_type_from_extension(&FileExtension::new("xyz")?),
            None
        );
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // parse_extension
    // ---------------------------------------------------------------------------

    #[test]
    fn parse_extension_simple() -> Result<(), S3GalleryError> {
        let ext = parse_extension("photo.jpg")
            .ok_or_else(|| S3GalleryError::Internal("expected Some".into()))?;
        assert_eq!(ext.as_str(), "jpg");
        Ok(())
    }

    #[test]
    fn parse_extension_no_extension() {
        assert!(parse_extension("README").is_none());
    }

    #[test]
    fn parse_extension_hidden_file() -> Result<(), S3GalleryError> {
        let ext = parse_extension(".gitignore")
            .ok_or_else(|| S3GalleryError::Internal("expected Some".into()))?;
        assert_eq!(ext.as_str(), "gitignore");
        Ok(())
    }

    #[test]
    fn parse_extension_empty() {
        assert!(parse_extension("").is_none());
    }

    #[test]
    fn parse_extension_double_extension() -> Result<(), S3GalleryError> {
        // Returns the last component only
        let ext = parse_extension("archive.tar.gz")
            .ok_or_else(|| S3GalleryError::Internal("expected Some".into()))?;
        assert_eq!(ext.as_str(), "gz");
        Ok(())
    }

    #[test]
    fn parse_extension_dot_dot() {
        assert!(parse_extension("..").is_none());
    }

    #[test]
    fn parse_extension_just_dot() {
        assert!(parse_extension(".").is_none());
    }

    #[test]
    fn parse_extension_uppercase() -> Result<(), S3GalleryError> {
        let ext = parse_extension("photo.JPG")
            .ok_or_else(|| S3GalleryError::Internal("expected Some".into()))?;
        // Should be normalized to lowercase
        assert_eq!(ext.as_str(), "jpg");
        Ok(())
    }

    #[test]
    fn parse_extension_whitespace() {
        assert!(parse_extension("  ").is_none());
    }
}
