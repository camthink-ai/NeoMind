//! Language Adaptation Test - Tests that LLM responds in user's language
//!
//! This test verifies that the Language Policy in system prompt works:
//! - User writes in Chinese → LLM responds in Chinese
//! - User writes in English → LLM responds in English
//! - Default: English for ambiguous input
//!
//! Run with: cargo test --test language_adaptation_test -- --ignored --nocapture
//!
//! Requires: Ollama running on localhost:11434 with a model (e.g., qwen2.5:3b)

use anyhow::Result;
use neomind_agent::session::SessionManager;
use neomind_llm::{OllamaConfig, OllamaRuntime};
use std::sync::Arc;
use std::time::Instant;

/// Check if Ollama is available
fn ollama_available() -> bool {
    use std::net::TcpStream;
    TcpStream::connect_timeout(
        &"127.0.0.1:11434".parse().unwrap(),
        std::time::Duration::from_secs(2),
    )
    .is_ok()
}

/// Test context
struct TestContext {
    session_manager: SessionManager,
    session_id: String,
}

impl TestContext {
    async fn new() -> Result<Self> {
        let model = std::env::var("MODEL")
            .unwrap_or_else(|_| "qwen2.5:3b".to_string());
        
        let endpoint = std::env::var("OLLAMA_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        
        println!("🔗 Using Ollama at {} with model {}", endpoint, model);
        
        let session_manager = SessionManager::memory();
        let session_id = session_manager.create_session().await?;
        
        let ollama_config = OllamaConfig {
            endpoint: endpoint.clone(),
            model: model.clone(),
            timeout_secs: 60,
        };
        
        let llm_runtime = Arc::new(OllamaRuntime::new(ollama_config)?);
        
        // Get the agent and set custom LLM
        let agent = session_manager.get_session(&session_id).await?;
        agent.set_custom_llm(llm_runtime).await;
        
        Ok(Self { session_manager, session_id })
    }
    
    async fn chat(&self, message: &str) -> Result<String> {
        let response = self.session_manager
            .process_message(&self.session_id, message)
            .await?;
        Ok(response.message.content)
    }
}

/// Detect if text is primarily Chinese
fn is_chinese(text: &str) -> bool {
    // Skip non-text markers like "✓" or tool execution indicators
    let text = text.replace("✓", "").replace("执行完成", "");
    
    let chinese_chars = text.chars()
        .filter(|c| matches!(c, '\u{4E00}'..='\u{9FFF}'))
        .count();
    let english_chars = text.chars()
        .filter(|c| c.is_ascii_alphabetic())
        .count();
    let total_chars = chinese_chars + english_chars;
    
    if total_chars == 0 {
        return false;
    }
    
    (chinese_chars as f32 / total_chars as f32) > 0.4
}

/// Detect if text is primarily English
fn is_english(text: &str) -> bool {
    // Skip non-text markers like "✓" or tool execution indicators
    let text = text.replace("✓", "");
    
    let chinese_chars = text.chars()
        .filter(|c| matches!(c, '\u{4E00}'..='\u{9FFF}'))
        .count();
    let english_chars = text.chars()
        .filter(|c| c.is_ascii_alphabetic())
        .count();
    let total_chars = chinese_chars + english_chars;
    
    if total_chars == 0 {
        return false;
    }
    
    (english_chars as f32 / total_chars as f32) > 0.5
}

#[tokio::test]
#[ignore = "Requires Ollama LLM backend. Run with: cargo test --test language_adaptation_test -- --ignored --nocapture"]
async fn test_chinese_input_chinese_response() -> Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }
    
    println!("\n=== Test: Chinese Input → Chinese Response ===\n");
    
    let ctx = TestContext::new().await?;
    
    // Test simple greeting in Chinese
    let start = Instant::now();
    let response = ctx.chat("你好，请介绍一下你自己").await?;
    let duration = start.elapsed();
    
    println!("📤 User: 你好，请介绍一下你自己");
    println!("📥 Assistant: {}", response);
    println!("⏱️  Duration: {:?}", duration);
    
    assert!(is_chinese(&response), 
        "Expected Chinese response but got: {}", response);
    
    println!("✅ Response is in Chinese as expected\n");
    
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama LLM backend. Run with: cargo test --test language_adaptation_test -- --ignored --nocapture"]
async fn test_english_input_english_response() -> Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }
    
    println!("\n=== Test: English Input → English Response ===\n");
    
    let ctx = TestContext::new().await?;
    
    // Test simple greeting in English
    let start = Instant::now();
    let response = ctx.chat("Hello, please introduce yourself").await?;
    let duration = start.elapsed();
    
    println!("📤 User: Hello, please introduce yourself");
    println!("📥 Assistant: {}", response);
    println!("⏱️  Duration: {:?}", duration);
    
    assert!(is_english(&response), 
        "Expected English response but got: {}", response);
    
    println!("✅ Response is in English as expected\n");
    
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama LLM backend. Run with: cargo test --test language_adaptation_test -- --ignored --nocapture"]
async fn test_multilingual_conversation() -> Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }
    
    println!("\n=== Test: Multilingual Conversation (Independent Sessions) ===\n");
    
    // Test 1: English question in its own session
    let ctx1 = TestContext::new().await?;
    let response1 = ctx1.chat("What is the weather like today?").await?;
    println!("📤 User (EN): What is the weather like today?");
    println!("📥 Assistant: {}\n", response1);
    // Note: Weather is not a tool, so LLM responds in English (default)
    assert!(is_english(&response1), "Expected English response, got: {}", response1);
    
    // Test 2: Chinese question in a NEW session (simulates new user interaction)
    let ctx2 = TestContext::new().await?;
    let response2 = ctx2.chat("今天天气怎么样？").await?;
    println!("📤 User (CN): 今天天气怎么样？");
    println!("📥 Assistant: {}\n", response2);
    // Each new session should respect user's language
    assert!(is_chinese(&response2), "Expected Chinese response, got: {}", response2);
    
    // Test 3: English again in new session
    let ctx3 = TestContext::new().await?;
    let response3 = ctx3.chat("What are IoT devices?").await?;
    println!("📤 User (EN): What are IoT devices?");
    println!("📥 Assistant: {}\n", response3);
    assert!(is_english(&response3), "Expected English response, got: {}", response3);
    
    println!("✅ All language tests passed with independent sessions\n");
    
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama LLM backend. Run with: cargo test --test language_adaptation_test -- --ignored --nocapture"]
async fn test_mixed_language_handling() -> Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }
    
    println!("\n=== Test: Mixed Language Handling ===\n");
    
    let ctx = TestContext::new().await?;
    
    // Test: Chinese with English technical terms
    let response = ctx.chat("请解释一下什么是 API 和 SDK").await?;
    
    println!("📤 User: 请解释一下什么是 API 和 SDK");
    println!("📥 Assistant: {}\n", response);
    
    // Should respond in Chinese even with English terms
    assert!(is_chinese(&response), 
        "Expected Chinese response for mixed input, got: {}", response);
    
    println!("✅ Correctly handled mixed language input\n");
    
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama LLM backend. Run with: cargo test --test language_adaptation_test -- --ignored --nocapture"]
async fn test_all_languages_comprehensive() -> Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }
    
    println!("\n========================================");
    println!("=== Language Adaptation Test Suite  ===");
    println!("========================================\n");
    
    let test_cases = vec![
        ("你好", "Chinese greeting", true),
        ("Hello", "English greeting", false),
        ("请帮我查一下设备状态", "Chinese IoT query", true),
        ("Please check the device status", "English IoT query", false),
        ("这个功能怎么用？", "Chinese question", true),
        ("How do I use this feature?", "English question", false),
    ];
    
    let mut passed = 0;
    let mut failed = 0;
    
    for (input, description, expect_chinese) in &test_cases {
        // Create a NEW session for each test case to ensure independence
        let ctx = TestContext::new().await?;
        
        let start = Instant::now();
        let response = ctx.chat(input).await?;
        let duration = start.elapsed();
        
        let is_chinese_response = is_chinese(&response);
        let is_english_response = is_english(&response);
        let correct = if *expect_chinese { is_chinese_response } else { is_english_response };
        
        println!("📝 Test: {}", description);
        println!("   Input: {}", input);
        println!("   Response ({}ms): {}...", duration.as_millis(), 
            response.chars().take(150).collect::<String>());
        println!("   Expected: {} | Got: {}", 
            if *expect_chinese { "Chinese" } else { "English" },
            if is_chinese_response { "Chinese" } else if is_english_response { "English" } else { "Mixed/Unknown" });
        println!("   Result: {}", if correct { "✅ PASS" } else { "❌ FAIL" });
        println!();
        
        if correct {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    
    println!("========================================");
    println!("Results: {} passed, {} failed", passed, failed);
    println!("========================================\n");
    
    // Allow at least 66% pass rate (LLM behavior can vary, 4/6 should pass)
    let total = passed + failed;
    let pass_rate = passed as f32 / total as f32;
    assert!(pass_rate >= 0.66, "Pass rate {}% is below 66%", (pass_rate * 100.0) as i32);
    
    Ok(())
}