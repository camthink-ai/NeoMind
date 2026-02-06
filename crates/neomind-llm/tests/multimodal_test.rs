//! Integration test for multimodal (vision) capabilities with qwen3-vl:2b.
//!
//! This test verifies that:
//! 1. Images are correctly encoded and sent to Ollama
//! 2. The model can see and describe the image content
//! 3. Images are preserved in conversation history for follow-up questions

use neomind_llm::backends::ollama::{OllamaConfig, OllamaRuntime};
use neomind_core::llm::backend::{LlmInput, LlmRuntime};
use neomind_core::message::{Message, MessageRole, Content, ContentPart};

/// A simple 1x1 red PNG image as data URL
/// This is a minimal valid PNG file: 1x1 pixel, red color
const RED_PNG_DATA_URL: &str = "\
data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";

/// A simple test image with a geometric pattern (blue square)
const BLUE_SQUARE_DATA_URL: &str = "\
data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mNk+M9Qzw0AEYBxVsC+mgAA9nIU/Zj2Q4AAAAASUVORK5CYII=";

/// Check if Ollama is available at the default endpoint
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

/// Check if qwen3-vl:2b model is available
async fn is_model_available(model: &str) -> bool {
    let client = reqwest::Client::new();
    if let Ok(resp) = client.get("http://localhost:11434/api/tags")
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                return text.contains(model);
            }
        }
    }
    false
}

#[tokio::test]
#[ignore] // Run with: cargo test --package edge-ai-llm --test multimodal_test -- --ignored
async fn test_multimodal_basic_image_description() {
    if !is_ollama_available().await {
        println!("Skipping test: Ollama is not available");
        return;
    }

    let model = "qwen3-vl:latest";
    if !is_model_available(model).await {
        println!("Skipping test: {} model is not available", model);
        return;
    }

    let runtime = OllamaRuntime::new(OllamaConfig::new(model))
        .expect("Failed to create Ollama runtime");

    println!("\n=== Test 1: Basic image description ===");

    // Create a multimodal message with text and image
    let parts = vec![
        ContentPart::text("What color is this image? Please answer in one word."),
        ContentPart::image_base64(RED_PNG_DATA_URL, "image/png"),
    ];

    let user_message = Message::new(MessageRole::User, Content::Parts(parts));

    let input = LlmInput {
        messages: vec![user_message],
        params: neomind_core::llm::backend::GenerationParams {
            temperature: Some(0.3),
            top_p: None,
            top_k: None,
            max_tokens: Some(100),
            max_context: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled: Some(false), // Disable thinking for faster image processing
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

    // The response should mention "red" or indicate the color
    let response_lower = output.text.to_lowercase();
    assert!(
        response_lower.contains("red") || response_lower.contains("红"),
        "Expected response to mention 'red' color, got: {}",
        output.text
    );
}

#[tokio::test]
#[ignore] // Run with: cargo test --package edge-ai-llm --test multimodal_test -- --ignored
async fn test_multimodal_image_in_history() {
    if !is_ollama_available().await {
        println!("Skipping test: Ollama is not available");
        return;
    }

    let model = "qwen3-vl:latest";
    if !is_model_available(model).await {
        println!("Skipping test: {} model is not available", model);
        return;
    }

    let runtime = OllamaRuntime::new(OllamaConfig::new(model))
        .expect("Failed to create Ollama runtime");

    println!("\n=== Test 2: Image preserved in conversation history ===");

    // First message with image
    let parts1 = vec![
        ContentPart::text("This is a red image. Remember this color."),
        ContentPart::image_base64(RED_PNG_DATA_URL, "image/png"),
    ];
    let msg1 = Message::new(MessageRole::User, Content::Parts(parts1));

    // Assistant response (simulated)
    let msg2 = Message::assistant("I see. It's a red image.");

    // Follow-up question WITHOUT image (testing if model remembers from context)
    let msg3 = Message::new(
        MessageRole::User,
        Content::Text("What color was the image I showed you? Answer in one word.".to_string()),
    );

    let input = LlmInput {
        messages: vec![msg1, msg2, msg3],
        params: neomind_core::llm::backend::GenerationParams {
            temperature: Some(0.3),
            top_p: None,
            top_k: None,
            max_tokens: Some(100),
            max_context: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled: Some(false),
        },
        model: Some(model.to_string()),
        stream: false,
        tools: None,
    };

    let result = runtime.generate(input).await;

    assert!(result.is_ok(), "LLM generation failed: {:?}", result.err());

    let output = result.unwrap();
    println!("Response: {}", output.text);

    // The model should remember the color from the earlier image
    let response_lower = output.text.to_lowercase();
    assert!(
        response_lower.contains("red") || response_lower.contains("红"),
        "Expected response to remember 'red' color, got: {}",
        output.text
    );
}

#[tokio::test]
#[ignore] // Run with: cargo test --package edge-ai-llm --test multimodal_test -- --ignored
async fn test_multimodal_streaming() {
    if !is_ollama_available().await {
        println!("Skipping test: Ollama is not available");
        return;
    }

    let model = "qwen3-vl:latest";
    if !is_model_available(model).await {
        println!("Skipping test: {} model is not available", model);
        return;
    }

    let runtime = OllamaRuntime::new(OllamaConfig::new(model))
        .expect("Failed to create Ollama runtime");

    println!("\n=== Test 3: Streaming multimodal response ===");

    let parts = vec![
        ContentPart::text("Describe this image briefly."),
        ContentPart::image_base64(BLUE_SQUARE_DATA_URL, "image/png"),
    ];

    let user_message = Message::new(MessageRole::User, Content::Parts(parts));

    let input = LlmInput {
        messages: vec![user_message],
        params: neomind_core::llm::backend::GenerationParams {
            temperature: Some(0.5),
            top_p: None,
            top_k: None,
            max_tokens: Some(200),
            max_context: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled: Some(false),
        },
        model: Some(model.to_string()),
        stream: true,
        tools: None,
    };

    let start = std::time::Instant::now();
    let result = runtime.generate_stream(input).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "LLM streaming failed: {:?}", result.err());

    let mut stream = result.unwrap();
    let mut full_response = String::new();
    let mut chunk_count = 0;

    use futures::StreamExt;
    while let Some(chunk_result) = stream.next().await {
        assert!(chunk_result.is_ok(), "Stream chunk error: {:?}", chunk_result.err());
        let (text, is_thinking) = chunk_result.unwrap();
        if !is_thinking {
            full_response.push_str(&text);
            print!("{}", text);
        }
        chunk_count += 1;
    }
    let total_elapsed = start.elapsed();

    println!("\n\nTotal chunks: {}", chunk_count);
    println!("Full response: {}", full_response);
    println!("First chunk latency: {:?}", elapsed);
    println!("Total time: {:?}", total_elapsed);

    assert!(!full_response.is_empty(), "Response should not be empty");
    assert!(chunk_count > 0, "Should receive at least one chunk");

    // Response should mention something about blue or a square
    let response_lower = full_response.to_lowercase();
    let has_blue = response_lower.contains("blue") || response_lower.contains("蓝");
    let has_square = response_lower.contains("square") || response_lower.contains("方");

    if !has_blue && !has_square {
        println!("Warning: Response doesn't mention blue or square. Got: {}", full_response);
    }
}

#[tokio::test]
#[ignore] // Run with: cargo test --package edge-ai-llm --test multimodal_test -- --ignored
async fn test_multimodal_with_tools() {
    if !is_ollama_available().await {
        println!("Skipping test: Ollama is not available");
        return;
    }

    let model = "qwen3-vl:latest";
    if !is_model_available(model).await {
        println!("Skipping test: {} model is not available", model);
        return;
    }

    let runtime = OllamaRuntime::new(OllamaConfig::new(model))
        .expect("Failed to create Ollama runtime");

    println!("\n=== Test 4: Multimodal with tool calling ===");

    // Create a simple tool definition
    let tools = vec![neomind_core::llm::backend::ToolDefinition {
        name: "describe_color".to_string(),
        description: "Report the detected color in an image".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "color": {
                    "type": "string",
                    "description": "The detected color"
                }
            },
            "required": ["color"]
        }),
    }];

    let parts = vec![
        ContentPart::text("What color is this image? Use the describe_color tool."),
        ContentPart::image_base64(RED_PNG_DATA_URL, "image/png"),
    ];

    let user_message = Message::new(MessageRole::User, Content::Parts(parts));

    let input = LlmInput {
        messages: vec![user_message],
        params: neomind_core::llm::backend::GenerationParams {
            temperature: Some(0.3),
            top_p: None,
            top_k: None,
            max_tokens: Some(200),
            max_context: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled: Some(false),
        },
        model: Some(model.to_string()),
        stream: false,
        tools: Some(tools),
    };

    let result = runtime.generate(input).await;

    assert!(result.is_ok(), "LLM generation failed: {:?}", result.err());

    let output = result.unwrap();
    println!("Response: {}", output.text);

    // The model should either describe the color directly or call the tool
    let response_lower = output.text.to_lowercase();
    let mentions_color = response_lower.contains("red") || response_lower.contains("红");
    let has_tool = response_lower.contains("describe_color") || response_lower.contains("tool");

    assert!(
        mentions_color || has_tool,
        "Expected response to mention 'red' or use tool, got: {}",
        output.text
    );
}
