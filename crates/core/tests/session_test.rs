//! Comprehensive tests for the Session module.
//!
//! Tests include:
//! - Session creation and management
//! - Message operations
//! - Metadata handling
//! - Context storage
//! - Session ID handling

use edge_ai_core::{
    message::{Message, MessageRole},
    session::{Session, SessionId},
};
use serde_json;

#[test]
fn test_session_id_new() {
    let id = SessionId::new();
    assert!(!id.as_str().is_empty());
    assert!(id.as_str().len() >= 16);
}

#[test]
fn test_session_id_from_string() {
    let s = "test-session-id";
    let id = SessionId::from_string(s.to_string());
    assert_eq!(id.as_str(), s);
}

#[test]
fn test_session_id_from_str() {
    let s = "another-session-id";
    let id = SessionId::from(s);
    assert_eq!(id.as_str(), s);
}

#[test]
fn test_session_id_default() {
    let id = SessionId::default();
    assert!(!id.as_str().is_empty());
}

#[test]
fn test_session_id_display() {
    let id = SessionId::from_string("test-id".to_string());
    assert_eq!(format!("{}", id), "test-id");
    assert_eq!(id.to_string(), "test-id");
}

#[test]
fn test_session_new() {
    let session = Session::new();
    assert!(session.is_empty());
    assert_eq!(session.len(), 0);
    assert!(session.id.as_str().len() > 0);
}

#[test]
fn test_session_with_id() {
    let id = SessionId::from_string("custom-id".to_string());
    let session = Session::with_id(id.clone());
    assert_eq!(session.id.as_str(), "custom-id");
}

#[test]
fn test_session_add_message() {
    let mut session = Session::new();
    session.add_message(Message::user("Hello"));

    assert_eq!(session.len(), 1);
    assert_eq!(session.messages[0].role, MessageRole::User);
    assert_eq!(session.messages[0].text(), "Hello");
}

#[test]
fn test_session_add_multiple_messages() {
    let mut session = Session::new();

    session.add_message(Message::user("Hello"));
    session.add_message(Message::assistant("Hi there!"));
    session.add_message(Message::user("How are you?"));

    assert_eq!(session.len(), 3);
    assert_eq!(session.messages[0].text(), "Hello");
    assert_eq!(session.messages[1].text(), "Hi there!");
    assert_eq!(session.messages[2].text(), "How are you?");
}

#[test]
fn test_session_clear() {
    let mut session = Session::new();

    session.add_message(Message::user("Hello"));
    session.add_message(Message::assistant("Hi!"));
    assert_eq!(session.len(), 2);

    session.clear();
    assert!(session.is_empty());
    assert_eq!(session.len(), 0);
}

#[test]
fn test_session_set_title() {
    let mut session = Session::new();

    assert!(session.metadata.title.is_none());

    session.set_title("Test Session");
    assert_eq!(session.metadata.title.as_ref().unwrap(), "Test Session");

    session.set_title("Updated Title");
    assert_eq!(session.metadata.title.as_ref().unwrap(), "Updated Title");
}

#[test]
fn test_session_set_model() {
    let mut session = Session::new();

    assert!(session.metadata.model.is_none());

    session.set_model("gpt-4");
    assert_eq!(session.metadata.model.as_ref().unwrap(), "gpt-4");

    session.set_model("claude-3");
    assert_eq!(session.metadata.model.as_ref().unwrap(), "claude-3");
}

#[test]
fn test_session_context() {
    let mut session = Session::new();

    // Initially empty
    assert!(session.get_context("key").is_none());

    // Set a value
    session.set_context("user_id", serde_json::json!("user123"));
    assert_eq!(
        session.get_context("user_id"),
        Some(&serde_json::json!("user123"))
    );

    // Update the value
    session.set_context("user_id", serde_json::json!("user456"));
    assert_eq!(
        session.get_context("user_id"),
        Some(&serde_json::json!("user456"))
    );

    // Multiple keys
    session.set_context("session_start", serde_json::json!(12345));
    session.set_context("theme", serde_json::json!("dark"));

    assert_eq!(session.get_context("session_start").unwrap(), &12345);
    assert_eq!(session.get_context("theme").unwrap(), &"dark");
}

#[test]
fn test_session_messages_ref() {
    let mut session = Session::new();

    session.add_message(Message::user("First"));
    session.add_message(Message::assistant("Second"));

    let messages = session.messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].text(), "First");
    assert_eq!(messages[1].text(), "Second");
}

#[test]
fn test_session_metadata_defaults() {
    let session = Session::new();

    assert!(session.metadata.title.is_none());
    assert!(session.metadata.model.is_none());
    assert!(session.metadata.created_at <= chrono::Utc::now());
    assert!(session.metadata.updated_at <= chrono::Utc::now());
}

#[test]
fn test_session_metadata_updated_on_change() {
    let mut session = Session::new();
    let original_time = session.metadata.updated_at;

    // Give it a millisecond
    std::thread::sleep(std::time::Duration::from_millis(10));

    session.set_title("New Title");

    assert!(session.metadata.updated_at > original_time);
}

#[test]
fn test_session_serialization() {
    let mut session = Session::new();
    session.set_title("Test Session");
    session.set_model("gpt-4");
    session.add_message(Message::user("Hello"));
    session.add_message(Message::assistant("Hi!"));
    session.set_context("key", serde_json::json!("value"));

    let json = serde_json::to_string(&session).expect("Failed to serialize");
    assert!(json.contains("Test Session"));
    assert!(json.contains("gpt-4"));
    assert!(json.contains("Hello"));

    let deserialized: Session = serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(deserialized.metadata.title, session.metadata.title);
    assert_eq!(deserialized.metadata.model, session.metadata.model);
    assert_eq!(deserialized.len(), session.len());
}

#[test]
fn test_session_clone() {
    let mut session = Session::new();
    session.set_title("Original");
    session.add_message(Message::user("Test"));

    let cloned = session.clone();

    assert_eq!(cloned.id, session.id);
    assert_eq!(cloned.metadata.title, session.metadata.title);
    assert_eq!(cloned.len(), session.len());
}

#[test]
fn test_session_default() {
    let session = Session::default();
    assert!(session.is_empty());
    assert!(session.id.as_str().len() > 0);
}

#[test]
fn test_session_with_multimodal_messages() {
    let mut session = Session::new();

    session.add_message(Message::user_with_image(
        "What's this?",
        "http://example.com/image.jpg",
    ));
    session.add_message(Message::assistant("That's a cat"));

    assert_eq!(session.len(), 2);
    assert!(session.messages[0].has_images());
    assert!(!session.messages[1].has_images());
}

#[test]
fn test_session_context_complex_values() {
    let mut session = Session::new();

    // Object value
    session.set_context(
        "user",
        serde_json::json!({
            "name": "Alice",
            "age": 30,
            "preferences": ["dark", "compact"]
        }),
    );

    let user = session.get_context("user").unwrap();
    assert_eq!(user["name"], "Alice");
    assert_eq!(user["age"], 30);
    assert_eq!(user["preferences"][0], "dark");

    // Array value
    session.set_context("history", serde_json::json!(["msg1", "msg2", "msg3"]));
    let history = session.get_context("history").unwrap();
    assert_eq!(history.as_array().unwrap().len(), 3);
}

#[test]
fn test_session_many_messages() {
    let mut session = Session::new();

    for i in 0..100 {
        if i % 2 == 0 {
            session.add_message(Message::user(format!("Message {}", i)));
        } else {
            session.add_message(Message::assistant(format!("Response {}", i)));
        }
    }

    assert_eq!(session.len(), 100);
    assert_eq!(session.messages[0].text(), "Message 0");
    assert_eq!(session.messages[99].text(), "Response 99");
}

#[test]
fn test_session_clear_preserves_metadata() {
    let mut session = Session::new();
    session.set_title("Test");
    session.set_model("gpt-4");
    session.set_context("key", serde_json::json!("value"));
    session.add_message(Message::user("Hello"));

    session.clear();

    assert!(session.is_empty());
    assert_eq!(session.metadata.title.as_ref().unwrap(), "Test");
    assert_eq!(session.metadata.model.as_ref().unwrap(), "gpt-4");
    assert_eq!(session.get_context("key").unwrap(), &serde_json::json!("value"));
}
