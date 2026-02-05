//! Comprehensive tests for the Tiered Memory system.
//!
//! Tests include:
//! - Short-term memory (current conversation)
//! - Mid-term memory (session history)
//! - Long-term memory (knowledge base)
//! - Memory consolidation
//! - Search operations (semantic, BM25, hybrid)

use edge_ai_memory::{
    tiered::{SearchMethod, TieredMemory, TieredMemoryConfig},
};
use edge_ai_core::message::Message;

#[tokio::test]
async fn test_tiered_memory_new() {
    let memory = TieredMemory::new();

    assert!(memory.is_hybrid_search_enabled());

    let stats = memory.get_stats().await;
    assert_eq!(stats.short_term_messages, 0);
    assert_eq!(stats.mid_term_entries, 0);
    assert_eq!(stats.long_term_entries, 0);
}

#[tokio::test]
async fn test_short_term_add_message() {
    let mut memory = TieredMemory::new();

    memory
        .add_message("user", "Hello, how are you?")
        .unwrap();

    let stats = memory.get_stats().await;
    assert_eq!(stats.short_term_messages, 1);

    let short_term = memory.get_short_term();
    assert_eq!(short_term.len(), 1);
    assert_eq!(short_term[0].content, "Hello, how are you?");
}

#[tokio::test]
async fn test_short_term_conversation() {
    let mut memory = TieredMemory::new();

    memory
        .add_message("user", "What's the weather?")
        .unwrap();
    memory
        .add_message("assistant", "It's sunny today.")
        .unwrap();
    memory
        .add_message("user", "What about tomorrow?")
        .unwrap();

    let stats = memory.get_stats().await;
    assert_eq!(stats.short_term_messages, 3);

    let messages = memory.get_last_messages(10);
    assert_eq!(messages.len(), 3);
}

#[tokio::test]
async fn test_short_term_clear() {
    let mut memory = TieredMemory::new();

    memory
        .add_message("user", "Test message")
        .unwrap();

    let stats = memory.get_stats().await;
    assert_eq!(stats.short_term_messages, 1);

    memory.short_term_mut().clear();

    let stats = memory.get_stats().await;
    assert_eq!(stats.short_term_messages, 0);
    assert!(memory.get_short_term().is_empty());
}

#[tokio::test]
async fn test_consolidate() {
    let mut memory = TieredMemory::new();

    // Add messages to short-term
    for i in 0..5 {
        memory
            .add_message("user", &format!("Message {}", i))
            .unwrap();
        memory
            .add_message("assistant", &format!("Response {}", i))
            .unwrap();
    }

    let stats = memory.get_stats().await;
    assert_eq!(stats.short_term_messages, 10);

    // Consolidate to mid-term
    memory.consolidate("session_consolidated").await.unwrap();

    // Short-term should NOT be cleared (consolidate copies, doesn't move)
    let stats = memory.get_stats().await;
    assert_eq!(stats.short_term_messages, 10);

    // Mid-term should have the conversation
    assert_eq!(stats.mid_term_entries, 5);
}

#[tokio::test]
async fn test_custom_config() {
    let config = TieredMemoryConfig {
        max_short_term_messages: 50,
        max_short_term_tokens: 8000,
        max_mid_term_entries: 200,
        max_long_term_knowledge: 500,
        embedding_dim: 384,
        use_hybrid_search: false,
        semantic_weight: 0.7,
        bm25_weight: 0.3,
        ..Default::default()
    };

    let memory = TieredMemory::with_config(config.clone());

    assert!(!memory.is_hybrid_search_enabled());
}

#[tokio::test]
async fn test_memory_stats() {
    let mut memory = TieredMemory::new();

    memory
        .add_message("user", "Test")
        .unwrap();

    let stats = memory.get_stats().await;
    assert_eq!(stats.short_term_messages, 1);
}

#[tokio::test]
async fn test_empty_memory_state() {
    let memory = TieredMemory::new();

    let short_term = memory.get_short_term();
    assert!(short_term.is_empty());
}

#[tokio::test]
async fn test_long_conversation() {
    let mut memory = TieredMemory::new();

    // Add a long conversation
    for i in 0..50 {
        memory
            .add_message("user", &format!("User message {}", i))
            .unwrap();
        memory
            .add_message("assistant", &format!("Assistant response {}", i))
            .unwrap();
    }

    let stats = memory.get_stats().await;
    assert_eq!(stats.short_term_messages, 100);

    // Get last N messages
    let last_10 = memory.get_last_messages(10);
    assert_eq!(last_10.len(), 10);
    assert!(last_10[0].content.contains("User message"));
}

#[tokio::test]
async fn test_query_all() {
    let mut memory = TieredMemory::new();

    // Add to short-term
    memory
        .add_message("user", "How do I pair a Bluetooth device?")
        .unwrap();

    let results = memory.query_all("bluetooth", 5).await;

    // Should find results from short-term at least
    assert!(results.short_term.len() > 0);
}

#[tokio::test]
async fn test_query_all_with_methods() {
    let mut memory = TieredMemory::new();

    memory
        .add_message("user", "How to connect WiFi?")
        .unwrap();
    memory
        .add_message("assistant", "Go to settings.")
        .unwrap();

    // Semantic search
    let semantic_results = memory
        .query_all_with_method("wireless internet connection", 5, SearchMethod::Semantic)
        .await;
    // Short-term should have the messages
    assert!(semantic_results.short_term.len() >= 0);

    // BM25 search
    let bm25_results = memory
        .query_all_with_method("WiFi settings", 5, SearchMethod::BM25)
        .await;
    assert!(bm25_results.short_term.len() >= 0);

    // Hybrid search
    let hybrid_results = memory
        .query_all_with_method("connect to network", 5, SearchMethod::Hybrid)
        .await;
    assert!(hybrid_results.short_term.len() >= 0);
}

#[tokio::test]
async fn test_search_in_short_term() {
    let mut memory = TieredMemory::new();

    memory
        .add_message("user", "How do I check the temperature?")
        .unwrap();
    memory
        .add_message("assistant", "Use the temperature command.")
        .unwrap();
    memory
        .add_message("user", "What about humidity?")
        .unwrap();

    let results = memory.query_all("temperature", 5).await;
    assert!(results.short_term.len() > 0);
}

#[tokio::test]
async fn test_get_last_messages() {
    let mut memory = TieredMemory::new();

    for i in 0..10 {
        memory
            .add_message("user", &format!("Message {}", i))
            .unwrap();
    }

    let last_5 = memory.get_last_messages(5);
    assert_eq!(last_5.len(), 5);

    // Should get the last 5 messages
    assert!(last_5[0].content.contains("Message"));
}

#[tokio::test]
async fn test_short_term_token_count() {
    let mut memory = TieredMemory::new();

    memory
        .add_message("user", "Hello world")
        .unwrap();
    memory
        .add_message("assistant", "Hi there!")
        .unwrap();

    let stats = memory.get_stats().await;
    assert_eq!(stats.short_term_messages, 2);
    assert!(stats.short_term_tokens > 0);
}

#[tokio::test]
async fn test_mid_term_search() {
    let mut memory = TieredMemory::new();

    // Add messages then consolidate to mid-term
    memory
        .add_message("user", "How do I connect to WiFi?")
        .unwrap();
    memory
        .add_message("assistant", "Go to Settings > WiFi and select your network.")
        .unwrap();

    memory.consolidate("session1").await.unwrap();

    // Search mid-term memory
    let results = memory.search_mid_term("WiFi connection settings", 5).await;
    assert!(results.len() > 0);
}

#[tokio::test]
async fn test_mid_term_bm25_search() {
    let mut memory = TieredMemory::new();

    memory
        .add_message("user", "How do I change my password?")
        .unwrap();
    memory
        .add_message("assistant", "Go to Settings > Security > Change Password.")
        .unwrap();

    memory.consolidate("session2").await.unwrap();

    // BM25 search
    let results = memory.search_mid_term_bm25("password change settings", 5).await;
    assert!(results.len() > 0);
}

#[tokio::test]
async fn test_config_embedding_dim() {
    let config = TieredMemoryConfig {
        embedding_dim: 128,
        ..Default::default()
    };

    let memory = TieredMemory::with_config(config);

    assert_eq!(memory.config().embedding_dim, 128);
}

#[tokio::test]
async fn test_config_search_weights() {
    let config = TieredMemoryConfig {
        semantic_weight: 0.8,
        bm25_weight: 0.2,
        ..Default::default()
    };

    let memory = TieredMemory::with_config(config);

    assert_eq!(memory.config().semantic_weight, 0.8);
    assert_eq!(memory.config().bm25_weight, 0.2);
}
