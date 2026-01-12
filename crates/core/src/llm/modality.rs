//! Multimodal content types for LLM interactions.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

/// Multimodal content for messages.
///
/// This enum represents different types of content that can be
/// sent to multimodal LLMs (text, images, audio, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModalityContent {
    /// Text content.
    Text(String),

    /// Image content.
    Image(ImageContent),

    /// Mixed content (multiple parts).
    Mixed(Vec<ModalityContent>),
}

impl ModalityContent {
    /// Create text content.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Create image content from URL.
    pub fn image_url(url: impl Into<String>) -> Self {
        Self::Image(ImageContent::Url { url: url.into() })
    }

    /// Create image content from base64 data.
    pub fn image_base64(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Image(ImageContent::Base64 {
            data: data.into(),
            mime_type: mime_type.into(),
        })
    }

    /// Create image content from file path.
    pub fn image_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        Ok(Self::Image(ImageContent::File {
            path: path.as_ref().to_path_buf(),
        }))
    }

    /// Create mixed content.
    pub fn mixed(parts: Vec<ModalityContent>) -> Self {
        Self::Mixed(parts)
    }

    /// Check if this content is text-only.
    pub fn is_text_only(&self) -> bool {
        matches!(self, Self::Text(_))
    }

    /// Check if this content contains images.
    pub fn has_images(&self) -> bool {
        match self {
            Self::Text(_) => false,
            Self::Image(_) => true,
            Self::Mixed(parts) => parts.iter().any(|p| p.has_images()),
        }
    }

    /// Get the text representation.
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Image(img) => format!("[Image: {}]", img.description()),
            Self::Mixed(parts) => parts.iter().map(|p| p.as_text()).collect::<Vec<_>>().join(" "),
        }
    }

    /// Get all text parts (excluding images).
    pub fn extract_text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Image(_) => String::new(),
            Self::Mixed(parts) => parts.iter().map(|p| p.extract_text()).collect::<Vec<_>>().join(" "),
        }
    }
}

impl From<String> for ModalityContent {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for ModalityContent {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

impl From<ImageContent> for ModalityContent {
    fn from(img: ImageContent) -> Self {
        Self::Image(img)
    }
}

impl fmt::Display for ModalityContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_text())
    }
}

/// Image content for multimodal messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ImageContent {
    /// Image from URL.
    #[serde(rename = "url")]
    Url {
        url: String,
    },

    /// Image from base64-encoded data.
    #[serde(rename = "base64")]
    Base64 {
        data: String,
        mime_type: String,
    },

    /// Image from file path.
    #[serde(rename = "file")]
    File {
        path: std::path::PathBuf,
    },

    /// Image from raw bytes.
    #[serde(rename = "bytes")]
    Bytes {
        data: Vec<u8>,
        mime_type: String,
    },
}

impl ImageContent {
    /// Get a description of this image source.
    pub fn description(&self) -> String {
        match self {
            Self::Url { url } => format!("URL: {}", url),
            Self::Base64 { mime_type, .. } => format!("Base64: {}", mime_type),
            Self::File { path } => format!("File: {}", path.display()),
            Self::Bytes { mime_type, .. } => format!("Bytes: {}", mime_type),
        }
    }

    /// Check if this is a URL image.
    pub fn is_url(&self) -> bool {
        matches!(self, Self::Url { .. })
    }

    /// Check if this is a file image.
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File { .. })
    }

    /// Get the MIME type if available.
    pub fn mime_type(&self) -> Option<&str> {
        match self {
            Self::Url { .. } => None, // URL content-type is determined by fetch
            Self::Base64 { mime_type, .. } => Some(mime_type),
            Self::File { .. } => None, // File MIME type determined by extension
            Self::Bytes { mime_type, .. } => Some(mime_type),
        }
    }
}

/// Input for image processing.
///
/// This is a lower-level representation used for backend
/// image processing before encoding.
#[derive(Debug, Clone)]
pub enum ImageInput {
    /// Image from file path.
    File(std::path::PathBuf),

    /// Image from URL.
    Url(String),

    /// Image from raw bytes.
    Bytes {
        data: Vec<u8>,
        format: Option<ImageFormat>,
    },
}

impl ImageInput {
    /// Create from file path.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Self {
        Self::File(path.as_ref().to_path_buf())
    }

    /// Create from URL.
    pub fn from_url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }

    /// Create from bytes.
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self::Bytes {
            data,
            format: None,
        }
    }

    /// Create from bytes with known format.
    pub fn from_bytes_with_format(data: Vec<u8>, format: ImageFormat) -> Self {
        Self::Bytes {
            data,
            format: Some(format),
        }
    }
}

/// Image format hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Gif,
    WebP,
    Bmp,
}

impl ImageFormat {
    /// Get MIME type for this format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Gif => "image/gif",
            Self::WebP => "image/webp",
            Self::Bmp => "image/bmp",
        }
    }

    /// Get common file extensions.
    pub fn extensions(&self) -> &[&str] {
        match self {
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Png => &["png"],
            Self::Gif => &["gif"],
            Self::WebP => &["webp"],
            Self::Bmp => &["bmp"],
        }
    }

    /// Detect format from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        let ext_lower = ext.to_lowercase();
        let ext = ext_lower.trim_start_matches('.');
        match ext {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::WebP),
            "bmp" => Some(Self::Bmp),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modality_content_text() {
        let content = ModalityContent::text("Hello, world!");
        assert!(content.is_text_only());
        assert!(!content.has_images());
        assert_eq!(content.as_text(), "Hello, world!");
    }

    #[test]
    fn test_modality_content_image() {
        let content = ModalityContent::image_url("https://example.com/image.jpg");
        assert!(!content.is_text_only());
        assert!(content.has_images());
    }

    #[test]
    fn test_mixed_content() {
        let content = ModalityContent::mixed(vec![
            ModalityContent::text("Describe this image:"),
            ModalityContent::image_url("https://example.com/image.jpg"),
        ]);
        assert!(!content.is_text_only());
        assert!(content.has_images());
        assert_eq!(content.extract_text(), "Describe this image: "); // Trailing space from join
    }

    #[test]
    fn test_image_format() {
        let format = ImageFormat::from_extension("jpg").unwrap();
        assert_eq!(format.mime_type(), "image/jpeg");
        assert_eq!(format, ImageFormat::Jpeg);
    }

    #[test]
    fn test_image_input() {
        let input = ImageInput::from_url("https://example.com/image.png");
        match input {
            ImageInput::Url(url) => assert_eq!(url, "https://example.com/image.png"),
            _ => panic!("Expected URL input"),
        }
    }
}
