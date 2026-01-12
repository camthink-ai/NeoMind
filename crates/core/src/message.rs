//! Message types for LLM interactions.

pub mod convert;

use serde::{Deserialize, Serialize};
use std::fmt;

/// Role of the message sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// System message - sets the behavior of the assistant.
    System,
    /// User message.
    User,
    /// Assistant message.
    Assistant,
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
        }
    }
}

/// Content of a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    /// Plain text content.
    Text(String),
    /// Structured content with multiple parts.
    Parts(Vec<ContentPart>),
}

impl Content {
    /// Create a new text content.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Get the text representation of this content.
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Parts(parts) => {
                parts.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("\n")
            }
        }
    }
}

impl From<String> for Content {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for Content {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

/// A part of structured content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    /// Text part.
    #[serde(rename = "text")]
    Text { text: String },

    /// Image part for multimodal models.
    #[serde(rename = "image_url")]
    ImageUrl {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },

    /// Image from base64 data.
    #[serde(rename = "image_base64")]
    ImageBase64 {
        data: String,
        mime_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
}

/// Image detail level for multimodal inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    /// Low detail (faster, less accurate)
    Low,
    /// High detail (slower, more accurate)
    High,
    /// Auto (default)
    Auto,
}

impl Default for ImageDetail {
    fn default() -> Self {
        Self::Auto
    }
}

impl ContentPart {
    /// Create a text part.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Create an image URL part.
    pub fn image_url(url: impl Into<String>) -> Self {
        Self::ImageUrl {
            url: url.into(),
            detail: None,
        }
    }

    /// Create an image URL part with detail level.
    pub fn image_url_with_detail(url: impl Into<String>, detail: ImageDetail) -> Self {
        Self::ImageUrl {
            url: url.into(),
            detail: Some(detail),
        }
    }

    /// Create an image from base64 data.
    pub fn image_base64(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::ImageBase64 {
            data: data.into(),
            mime_type: mime_type.into(),
            detail: None,
        }
    }

    /// Check if this part contains an image.
    pub fn is_image(&self) -> bool {
        matches!(self, Self::ImageUrl { .. } | Self::ImageBase64 { .. })
    }
}

impl fmt::Display for ContentPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text { text } => write!(f, "{}", text),
            Self::ImageUrl { url, .. } => write!(f, "[Image: {}]", url),
            Self::ImageBase64 { mime_type, .. } => write!(f, "[Image: {}]", mime_type),
        }
    }
}

/// A chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Role of the sender.
    pub role: MessageRole,
    /// Content of the message.
    pub content: Content,
    /// Optional timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

impl Message {
    /// Create a new message.
    pub fn new(role: MessageRole, content: impl Into<Content>) -> Self {
        Self {
            role,
            content: content.into(),
            timestamp: Some(chrono::Utc::now()),
        }
    }

    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(MessageRole::System, Content::text(content))
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(MessageRole::User, Content::text(content))
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(MessageRole::Assistant, Content::text(content))
    }

    /// Create a multimodal user message with text and image.
    pub fn user_with_image(text: impl Into<String>, image_url: impl Into<String>) -> Self {
        Self::new(
            MessageRole::User,
            Content::Parts(vec![
                ContentPart::text(text),
                ContentPart::image_url(image_url),
            ]),
        )
    }

    /// Create a multimodal user message with text, image URL and detail level.
    pub fn user_with_image_detail(
        text: impl Into<String>,
        image_url: impl Into<String>,
        detail: ImageDetail,
    ) -> Self {
        Self::new(
            MessageRole::User,
            Content::Parts(vec![
                ContentPart::text(text),
                ContentPart::image_url_with_detail(image_url, detail),
            ]),
        )
    }

    /// Create a user message with base64 image.
    pub fn user_with_image_base64(
        text: impl Into<String>,
        image_data: impl Into<String>,
        mime_type: impl Into<String>,
    ) -> Self {
        Self::new(
            MessageRole::User,
            Content::Parts(vec![
                ContentPart::text(text),
                ContentPart::image_base64(image_data, mime_type),
            ]),
        )
    }

    /// Create a message from parts.
    pub fn from_parts(role: MessageRole, parts: Vec<ContentPart>) -> Self {
        Self::new(role, Content::Parts(parts))
    }

    /// Get the text content.
    pub fn text(&self) -> String {
        self.content.as_text()
    }

    /// Check if this message contains images.
    pub fn has_images(&self) -> bool {
        match &self.content {
            Content::Text(_) => false,
            Content::Parts(parts) => parts.iter().any(|p| p.is_image()),
        }
    }

    /// Add a part to this message (converts to Parts if needed).
    pub fn with_part(mut self, part: ContentPart) -> Self {
        match &mut self.content {
            Content::Parts(parts) => {
                parts.push(part);
            }
            Content::Text(text) => {
                self.content = Content::Parts(vec![
                    ContentPart::text(std::mem::take(text)),
                    part,
                ]);
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = Message::user("Hello, world!");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.text(), "Hello, world!");
    }
}
