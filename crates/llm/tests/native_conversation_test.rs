//! Native backend conversation test - tests real model output quality and performance
//!
//! Run with: cargo test -p edge-ai-llm --test native_conversation_test --features native -- --nocapture --test-threads=1

use std::io::Write;
use std::sync::Arc;
use edge_ai_llm::backends::{NativeConfig, NativeRuntime};
use edge_ai_core::{
    llm::backend::{LlmRuntime, LlmInput, GenerationParams},
    Message,
};
use futures::StreamExt;

#[tokio::test]
async fn test_native_basic_conversation() {
    // Initialize logging
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init();

    println!("\n{:=^80}", "");
    println!(" NATIVE BACKEND - BASIC CONVERSATION TEST");
    println!(" Model: qwen3:1.7b (Qwen2.5-1.5B-Instruct from HuggingFace)");
    println!("{:=^80}\n", "");

    // Configure native backend
    let config = NativeConfig::new("qwen3:1.7b")
        .with_device("cpu")
        .with_max_seq_len(2048);

    println!("⏳ Loading model... (this may take a while on first run)");
    let runtime = NativeRuntime::new(config).expect("Failed to create runtime");
    let runtime = Arc::new(runtime);

    // Check availability
    println!("🔍 Checking model availability...");
    let available = runtime.is_available().await;
    if !available {
        println!("❌ Model is not available.");
        println!("   This test requires downloading a ~3GB model from HuggingFace.");
        println!("   Model: Qwen/Qwen2.5-1.5B-Instruct");
        println!("   Cache location: ~/.cache/neotalk/models/qwen3-1.7b/");
        println!("\n   To test with Ollama instead (faster, already installed), run:");
        println!("   cargo test -p edge-ai-llm --test conversation_test -- --nocapture");
        println!("\n   Skipping native test for now...");
        return;
    }
    println!("✅ Model is available!\n");

    // Test cases for basic conversation
    let test_cases = vec![
        ("简单问候", "你好，请自我介绍一下。"),
        ("数学计算", "25 + 37 等于多少？"),
        ("常识问答", "中国的首都是哪里？"),
        ("英文对话", "Hello, how are you today?"),
    ];

    let mut total_tests = 0;
    let mut passed_tests = 0;

    for (test_name, user_message) in &test_cases {
        total_tests += 1;
        println!("\n{:-^80}", "");
        println!(" [{}/{}] {}", total_tests, test_cases.len(), test_name);
        println!("{:-^80}", "");
        println!("用户: {}\n", user_message);

        let input = LlmInput {
            messages: vec![Message::user(*user_message)],
            params: GenerationParams {
                max_tokens: Some(512),
                temperature: Some(0.7),
                ..Default::default()
            },
            model: Some("qwen3:1.7b".to_string()),
            stream: true,
            tools: None,
        };

        let mut full_response = String::new();
        let mut chunk_count = 0usize;
        let start_time = std::time::Instant::now();

        match runtime.generate_stream(input).await {
            Ok(mut stream) => {
                print!("助手: ");
                std::io::stdout().flush().unwrap();

                loop {
                    match stream.next().await {
                        Some(chunk_result) => match chunk_result {
                            Ok((text, is_thinking)) => {
                                chunk_count += 1;
                                if !is_thinking {
                                    print!("{}", text);
                                    std::io::stdout().flush().unwrap();
                                    full_response.push_str(&text);
                                }
                            }
                            Err(e) => {
                                println!("\n❌ 流错误: {}", e);
                                break;
                            }
                        }
                        None => break,
                    }
                }

                let elapsed = start_time.elapsed();

                println!("\n\n📊 统计:");
                println!("  ⏱️  用时: {:.2}s", elapsed.as_secs_f64());
                println!("  📦 接收块数: {}", chunk_count);
                println!("  📝 回复字符: {}", full_response.chars().count());

                if full_response.chars().count() > 10 {
                    println!("  ✅ 测试通过");
                    passed_tests += 1;
                } else {
                    println!("  ⚠️  回复较短");
                }
            }
            Err(e) => {
                println!("❌ 请求失败: {}", e);
            }
        }
    }

    println!("\n{:=^80}", "");
    println!(" 测试汇总");
    println!("{:=^80}", "");
    println!("  总测试数: {}", total_tests);
    println!("  通过: {} ✅", passed_tests);
    println!("  成功率: {:.1}%", (passed_tests as f64 / total_tests as f64 * 100.0));
    println!("{:=^80}\n", "");

    assert!(passed_tests >= total_tests / 2, "At least half of the tests should pass");
}

#[tokio::test]
async fn test_native_conversation_with_history() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init();

    println!("\n{:=^80}", "");
    println!(" NATIVE BACKEND - CONVERSATION WITH HISTORY TEST");
    println!("{:=^80}\n", "");

    let config = NativeConfig::new("qwen3:1.7b")
        .with_device("cpu")
        .with_max_seq_len(2048);

    let runtime = Arc::new(NativeRuntime::new(config).expect("Failed to create runtime"));

    // Check if model is available
    if !runtime.is_available().await {
        println!("⚠️  Model not available, skipping test. Run test_basic_conversation first.");
        return;
    }

    // Multi-turn conversation
    let mut messages = vec![
        Message::user("我叫小明，是一名程序员，喜欢打篮球。"),
    ];

    println!("📝 初始信息: 我叫小明，是一名程序员，喜欢打篮球。\n");

    for (turn, user_msg) in [
        "你还记得我的名字吗？",
        "我的职业是什么？",
        "我喜欢什么运动？",
        "请总结一下我的信息",
    ].iter().enumerate() {
        println!("\n{:-^80}", "");
        println!(" 第 {} 轮对话", turn + 1);
        println!("{:-^80}", "");
        println!("用户: {}", user_msg);

        messages.push(Message::user(*user_msg));

        let input = LlmInput {
            messages: messages.clone(),
            params: GenerationParams {
                max_tokens: Some(512),
                temperature: Some(0.7),
                ..Default::default()
            },
            model: Some("qwen3:1.7b".to_string()),
            stream: true,
            tools: None,
        };

        let start_time = std::time::Instant::now();

        match runtime.generate_stream(input).await {
            Ok(mut stream) => {
                let mut response = String::new();
                print!("助手: ");
                std::io::stdout().flush().unwrap();

                loop {
                    match stream.next().await {
                        Some(Ok((text, is_thinking))) => {
                            if !is_thinking {
                                print!("{}", text);
                                std::io::stdout().flush().unwrap();
                                response.push_str(&text);
                            }
                        }
                        Some(Err(e)) => {
                            println!("\n❌ 错误: {}", e);
                            break;
                        }
                        None => break,
                    }
                }

                let elapsed = start_time.elapsed();
                println!("\n  (⏱️ {:.2}s, 📝 {} 字)", elapsed.as_secs_f64(), response.chars().count());

                messages.push(Message::assistant(&response));
            }
            Err(e) => {
                println!("❌ 请求失败: {}", e);
            }
        }
    }

    println!("\n{:=^80}", "");
    println!(" 多轮对话测试完成");
    println!("{:=^80}\n", "");
}

#[tokio::test]
async fn test_native_with_tools() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init();

    println!("\n{:=^80}", "");
    println!(" NATIVE BACKEND - TOOL CALLING TEST");
    println!("{:=^80}\n", "");

    let config = NativeConfig::new("qwen3:1.7b")
        .with_device("cpu")
        .with_max_seq_len(2048);

    let runtime = Arc::new(NativeRuntime::new(config).expect("Failed to create runtime"));

    // Check if model is available
    if !runtime.is_available().await {
        println!("⚠️  Model not available, skipping test.");
        return;
    }

    use edge_ai_core::llm::backend::ToolDefinition;
    use serde_json::json;

    let tools = vec![
        ToolDefinition {
            name: "get_weather".to_string(),
            description: "获取指定城市的天气信息".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "city": {
                        "type": "string",
                        "description": "城市名称"
                    }
                },
                "required": ["city"]
            }),
        },
        ToolDefinition {
            name: "get_time".to_string(),
            description: "获取当前时间".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    ];

    let user_message = "请问北京今天天气怎么样？现在几点了？";
    println!("用户: {}\n", user_message);

    let input = LlmInput {
        messages: vec![
            Message::user(user_message),
        ],
        params: GenerationParams {
            max_tokens: Some(512),
            temperature: Some(0.7),
            ..Default::default()
        },
        model: Some("qwen3:1.7b".to_string()),
        stream: true,
        tools: Some(tools),
    };

    let start_time = std::time::Instant::now();

    match runtime.generate_stream(input).await {
        Ok(mut stream) => {
            let mut response = String::new();
            print!("助手: ");
            std::io::stdout().flush().unwrap();

            loop {
                match stream.next().await {
                    Some(Ok((text, is_thinking))) => {
                        if !is_thinking {
                            print!("{}", text);
                            std::io::stdout().flush().unwrap();
                            response.push_str(&text);
                        }
                    }
                    Some(Err(e)) => {
                        println!("\n❌ 错误: {}", e);
                        break;
                    }
                    None => break,
                }
            }

            let elapsed = start_time.elapsed();
            println!("\n\n📊 统计:");
            println!("  ⏱️  用时: {:.2}s", elapsed.as_secs_f64());
            println!("  📝 回复字符: {}", response.chars().count());

            // Check if tool calling format is present
            if response.contains("get_weather") || response.contains("get_time") {
                println!("  ✅ 模型尝试调用工具");
            } else {
                println!("  ⚠️  模型没有使用工具调用格式");
            }
        }
        Err(e) => {
            println!("❌ 请求失败: {}", e);
        }
    }

    println!("\n{:=^80}\n", "");
}
