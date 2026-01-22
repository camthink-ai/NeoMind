//! Unit test for multimodal message handling.
//!
//! This test verifies that:
//! 1. AgentMessage correctly stores images
//! 2. AgentMessage::to_core() correctly converts to multimodal Message
//! 3. Images are preserved in the conversion

use edge_ai_agent::agent::types::{AgentMessage, AgentMessageImage};
use edge_ai_core::message::{Message, MessageRole, Content, ContentPart};

#[test]
fn test_agent_message_with_images() {
    // Create a user message with images
    let images = vec![
        AgentMessageImage {
            data: "data:image/png;base64,iVBORw0KGgo...".to_string(),
            mime_type: Some("image/png".to_string()),
        },
    ];

    let msg = AgentMessage::user_with_images("What color is this image?", images);

    // Verify the message was created correctly
    assert_eq!(msg.role, "user");
    assert_eq!(msg.content, "What color is this image?");
    assert!(msg.images.is_some());
    assert_eq!(msg.images.as_ref().unwrap().len(), 1);
}

#[test]
fn test_agent_message_to_core_multimodal() {
    // Create a user message with images
    let images = vec![
        AgentMessageImage {
            data: "data:image/png;base64,iVBORw0KGgo...".to_string(),
            mime_type: Some("image/png".to_string()),
        },
    ];

    let msg = AgentMessage::user_with_images("Describe this image", images);

    // Convert to core Message
    let core_msg = msg.to_core();

    // Verify it's a user message
    assert_eq!(core_msg.role, MessageRole::User);

    // Verify content is Parts (multimodal)
    match &core_msg.content {
        Content::Parts(parts) => {
            // Should have 2 parts: text + image
            assert_eq!(parts.len(), 2, "Expected 2 parts (text + image)");

            // First part should be text
            match &parts[0] {
                ContentPart::Text { text } => {
                    assert_eq!(text, "Describe this image");
                }
                _ => panic!("First part should be Text"),
            }

            // Second part should be image
            match &parts[1] {
                ContentPart::ImageBase64 { data, .. } => {
                    assert_eq!(data, "iVBORw0KGgo...");
                }
                _ => panic!("Second part should be ImageBase64"),
            }
        }
        _ => panic!("Expected Content::Parts for multimodal message"),
    }
}

#[test]
fn test_agent_message_without_images() {
    // Create a regular user message without images
    let msg = AgentMessage::user("Hello, how are you?");

    // Convert to core Message
    let core_msg = msg.to_core();

    // Verify it's a simple text message
    assert_eq!(core_msg.role, MessageRole::User);

    match &core_msg.content {
        Content::Text(text) => {
            assert_eq!(text, "Hello, how are you?");
        }
        _ => panic!("Expected Content::Text for simple message"),
    }
}

#[test]
fn test_agent_message_assistant_preserves_content() {
    // Create an assistant message
    let msg = AgentMessage::assistant("The image shows a red square.");

    // Convert to core Message
    let core_msg = msg.to_core();

    // Verify content is preserved
    assert_eq!(core_msg.role, MessageRole::Assistant);
    match &core_msg.content {
        Content::Text(text) => {
            assert_eq!(text, "The image shows a red square.");
        }
        _ => panic!("Expected Content::Text for assistant message"),
    }
}

#[test]
fn test_multiple_images_preserved() {
    // Create a message with multiple images
    let images = vec![
        AgentMessageImage {
            data: "data:image/png;base64,AAAA...".to_string(),
            mime_type: Some("image/png".to_string()),
        },
        AgentMessageImage {
            data: "data:image/jpeg;base64,BBBB...".to_string(),
            mime_type: Some("image/jpeg".to_string()),
        },
    ];

    let msg = AgentMessage::user_with_images("Compare these two images", images);

    // Convert to core Message
    let core_msg = msg.to_core();

    // Verify all images are preserved
    match &core_msg.content {
        Content::Parts(parts) => {
            // Should have 3 parts: text + 2 images
            assert_eq!(parts.len(), 3, "Expected 3 parts (text + 2 images)");
        }
        _ => panic!("Expected Content::Parts for multimodal message"),
    }
}

#[test]
fn test_message_image_data_url_parsing() {
    // Test various data URL formats
    let test_cases = vec![
        ("data:image/png;base64,iVBORw0...", "image/png", "iVBORw0..."),
        ("data:image/jpeg;base64,/9j/4AAQ...", "image/jpeg", "/9j/4AAQ..."),
        ("data:image/webp;base64,UklGR...", "image/webp", "UklGR..."),
    ];

    for (data_url, expected_mime, expected_data) in test_cases {
        let images = vec![AgentMessageImage {
            data: data_url.to_string(),
            mime_type: None, // Will be parsed from data URL
        }];

        let msg = AgentMessage::user_with_images("Test", images);
        let core_msg = msg.to_core();

        match &core_msg.content {
            Content::Parts(parts) => {
                if let Some(ContentPart::ImageBase64 { data, mime_type, .. }) = parts.get(1) {
                    assert_eq!(data, expected_data, "Data mismatch for {}", data_url);
                    assert_eq!(mime_type, &expected_mime.to_string(), "MIME type mismatch for {}", data_url);
                } else {
                    panic!("Expected ImageBase64 part");
                }
            }
            _ => panic!("Expected Content::Parts"),
        }
    }
}
