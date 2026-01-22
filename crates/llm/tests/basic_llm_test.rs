//! Basic LLM test to verify connectivity.

use edge_ai_llm::backends::ollama::{OllamaConfig, OllamaRuntime};
use edge_ai_core::llm::backend::{LlmInput, LlmRuntime};
use edge_ai_core::message::{Message, MessageRole, Content};

async fn is_ollama_available() -> bool {
    let client = reqwest::Client::new();
    if let Ok(resp) = client.get("http://localhost:11434/api/tags")
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        resp.status().is_success()
    } else {
        false
    }
}

#[tokio::test]
#[ignore]
async fn test_basic_chat() {
    if !is_ollama_available().await {
        println!("Skipping test: Ollama is not available");
        return;
    }

    let model = "qwen3:1.7b";
    let runtime = OllamaRuntime::new(OllamaConfig::new(model))
        .expect("Failed to create Ollama runtime");

    println!("\n=== Test: Basic chat ===");

    let user_message = Message::new(
        MessageRole::User,
        Content::Text("What is 2+2? Answer in one word.".to_string()),
    );

    let input = LlmInput {
        messages: vec![user_message],
        params: edge_ai_core::llm::backend::GenerationParams {
            temperature: Some(0.3),
            top_p: None,
            top_k: None,
            max_tokens: Some(50),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled: Some(false),
        },
        model: Some(model.to_string()),
        stream: false,
        tools: None,
    };

    let start = std::time::Instant::now();
    let result = runtime.generate(input).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "LLM generation failed: {:?}", result.err());

    let output = result.unwrap();
    println!("Response: {}", output.text);
    println!("Latency: {:?}", elapsed);

    let response_lower = output.text.to_lowercase();
    assert!(
        response_lower.contains("4"),
        "Expected response to mention '4', got: {}",
        output.text
    );
}
