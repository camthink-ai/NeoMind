//! Comprehensive tests for the Message module.
//!
//! Tests include:
//! - Message creation with various roles
//! - Multimodal content (text + images)
//! - Content serialization/deserialization
//! - Content part utilities
//! - Image detail levels

use edge_ai_core::{
    message::{Content, ContentPart, ImageDetail, Message, MessageRole},
};
use serde_json;

#[test]
fn test_message_user_creation() {
    let msg = Message::user("Hello, world!");
    assert_eq!(msg.role, MessageRole::User);
    assert_eq!(msg.text(), "Hello, world!");
    assert!(!msg.has_images());
    assert!(msg.timestamp.is_some());
}

#[test]
fn test_message_system_creation() {
    let msg = Message::system("You are a helpful assistant.");
    assert_eq!(msg.role, MessageRole::System);
    assert_eq!(msg.text(), "You are a helpful assistant.");
}

#[test]
fn test_message_assistant_creation() {
    let msg = Message::assistant("I can help you with that.");
    assert_eq!(msg.role, MessageRole::Assistant);
    assert_eq!(msg.text(), "I can help you with that.");
}

#[test]
fn test_message_with_image() {
    let msg = Message::user_with_image("What's in this image?", "http://example.com/image.jpg");
    assert_eq!(msg.role, MessageRole::User);
    assert!(msg.has_images());
    // text() method returns combined text with image placeholder
    assert!(msg.text().contains("What's in this image?"));
}

#[test]
fn test_message_with_image_detail() {
    let msg = Message::user_with_image_detail(
        "Describe this image",
        "http://example.com/image.jpg",
        ImageDetail::High,
    );
    assert!(msg.has_images());

    match &msg.content {
        Content::Parts(parts) => {
            assert_eq!(parts.len(), 2);
            assert!(!parts[0].is_image()); // Text part
            assert!(parts[1].is_image()); // Image part
        }
        _ => panic!("Expected Parts content"),
    }
}

#[test]
fn test_message_with_base64_image() {
    let msg = Message::user_with_image_base64(
        "What is this?",
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
        "image/png",
    );
    assert!(msg.has_images());

    match &msg.content {
        Content::Parts(parts) => {
            assert_eq!(parts.len(), 2);
            match &parts[1] {
                ContentPart::ImageBase64 { mime_type, .. } => {
                    assert_eq!(mime_type, "image/png");
                }
                other => panic!("Expected ImageBase64, got {:?}", other),
            }
        }
        _ => panic!("Expected Parts content"),
    }
}

#[test]
fn test_message_from_parts() {
    let parts = vec![
        ContentPart::text("Hello"),
        ContentPart::image_url("http://example.com/1.jpg"),
        ContentPart::image_url("http://example.com/2.jpg"),
    ];

    let msg = Message::from_parts(MessageRole::User, parts);
    assert_eq!(msg.role, MessageRole::User);
    assert!(msg.has_images());

    match &msg.content {
        Content::Parts(parts) => {
            assert_eq!(parts.len(), 3);
        }
        _ => panic!("Expected Parts content"),
    }
}

#[test]
fn test_message_with_part() {
    let msg = Message::user("Hello")
        .with_part(ContentPart::image_url("http://example.com/image.jpg"));

    assert!(msg.has_images());

    match &msg.content {
        Content::Parts(parts) => {
            assert_eq!(parts.len(), 2);
        }
        _ => panic!("Expected Parts content"),
    }
}

#[test]
fn test_content_from_string() {
    let content: Content = "Hello, world!".into();
    assert_eq!(content.as_text(), "Hello, world!");

    let content: Content = String::from("Test").into();
    assert_eq!(content.as_text(), "Test");
}

#[test]
fn test_content_part_text() {
    let part = ContentPart::text("Hello");
    assert!(!part.is_image());
    assert_eq!(part.to_string(), "Hello");
}

#[test]
fn test_content_part_image_url() {
    let part = ContentPart::image_url("http://example.com/image.jpg");
    assert!(part.is_image());
    assert_eq!(part.to_string(), "[Image: http://example.com/image.jpg]");
}

#[test]
fn test_content_part_image_url_with_detail() {
    let part = ContentPart::image_url_with_detail("http://example.com/image.jpg", ImageDetail::Low);
    assert!(part.is_image());
}

#[test]
fn test_content_part_image_base64() {
    let part = ContentPart::image_base64("base64data", "image/jpeg");
    assert!(part.is_image());
    assert_eq!(part.to_string(), "[Image: image/jpeg]");
}

#[test]
fn test_message_serialization() {
    let msg = Message::user_with_image("Check this out", "http://example.com/image.jpg");

    let json = serde_json::to_string(&msg).expect("Failed to serialize");
    // Check for lowercase role (serde serialization) and content
    assert!(json.contains("\"role\":\"user\""));
    // Content with images serializes as an array
    assert!(json.contains("[") || json.contains("\"parts\""));

    let deserialized: Message = serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(deserialized.role, MessageRole::User);
    assert!(deserialized.has_images());
}

#[test]
fn test_message_role_display() {
    assert_eq!(MessageRole::System.to_string(), "system");
    assert_eq!(MessageRole::User.to_string(), "user");
    assert_eq!(MessageRole::Assistant.to_string(), "assistant");
}

#[test]
fn test_empty_message() {
    let msg = Message::new(MessageRole::User, Content::text(""));
    assert_eq!(msg.text(), "");
}

#[test]
fn test_multimodal_text_extraction() {
    // Text content
    let msg1 = Message::user("Just text");
    assert_eq!(msg1.text(), "Just text");

    // Multimodal content - text extraction includes image placeholder
    let msg2 = Message::user_with_image("Text with image", "http://example.com/image.jpg");
    let text = msg2.text();
    assert!(text.contains("Text with image"));
    assert!(text.contains("[Image:"));
}

#[test]
fn test_content_part_image_detail_variants() {
    let low = ContentPart::image_url_with_detail("http://example.com/image.jpg", ImageDetail::Low);
    let high = ContentPart::image_url_with_detail("http://example.com/image.jpg", ImageDetail::High);
    let auto = ContentPart::image_url_with_detail("http://example.com/image.jpg", ImageDetail::Auto);

    // All should be images
    assert!(low.is_image());
    assert!(high.is_image());
    assert!(auto.is_image());
}

#[test]
fn test_message_timestamp_default() {
    let msg = Message::user("Test");
    assert!(msg.timestamp.is_some());
}

#[test]
fn test_message_with_no_timestamp() {
    let mut msg = Message::user("Test");
    msg.timestamp = None;
    assert!(msg.timestamp.is_none());
}

#[test]
fn test_multiple_images() {
    let msg = Message::from_parts(
        MessageRole::User,
        vec![
            ContentPart::text("Describe these images"),
            ContentPart::image_url("http://example.com/1.jpg"),
            ContentPart::image_url("http://example.com/2.jpg"),
            ContentPart::image_url("http://example.com/3.jpg"),
        ],
    );

    assert!(msg.has_images());

    match &msg.content {
        Content::Parts(parts) => {
            assert_eq!(parts.len(), 4);
            assert_eq!(parts.iter().filter(|p| p.is_image()).count(), 3);
        }
        _ => panic!("Expected Parts content"),
    }
}

#[test]
fn test_message_clone() {
    let msg = Message::user_with_image("Test", "http://example.com/image.jpg");
    let cloned = msg.clone();

    assert_eq!(msg.role, cloned.role);
    assert_eq!(msg.text(), cloned.text());
    assert_eq!(msg.has_images(), cloned.has_images());
}

#[test]
fn test_message_equality() {
    let msg1 = Message::user("Hello");
    let msg2 = Message::user("Hello");
    let msg3 = Message::user("Goodbye");

    // Messages derive PartialEq, so we can compare them
    // Note: timestamps will differ due to creation time
    assert_eq!(msg1.role, msg2.role);
    assert_eq!(msg1.text(), msg2.text());
    assert_ne!(msg1.text(), msg3.text());
}
