//! Agent Performance Test Suite
//!
//! Tests for:
//! 1. Complex conversation handling
//! 2. Multi-tool calling performance
//! 3. Intent understanding and task planning
//!
//! Run with: cargo test -p edge-ai-agent --test agent_performance_test -- --nocapture

use std::sync::Arc;
use std::time::Instant;
use futures::StreamExt;
use edge_ai_core::{
    llm::backend::{LlmRuntime, LlmInput, GenerationParams},
    message::Message,
};
use edge_ai_llm::{OllamaConfig, OllamaRuntime};

#[derive(Clone)]
struct TestMetrics {
    tool_calls_count: usize,
    thinking_chars: usize,
    content_chars: usize,
    execution_time_ms: u64,
    intent_correct: bool,
    planning_quality: f32,
}

/// Test 1: Intent Understanding
/// Tests how well the agent understands various types of user requests
#[tokio::test]
async fn test_intent_understanding() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init();

    println!("\n{:=^80}", "");
    println!(" INTENT UNDERSTANDING TEST");
    println!("{:=^80}\n", "");

    let config = OllamaConfig::new("qwen3-vl:2b")
        .with_endpoint("http://localhost:11434");
    let runtime = Arc::new(OllamaRuntime::new(config).expect("Failed to create runtime"));

    // Test cases with expected intent classification
    let test_cases = vec![
        // Query Intent
        ("查询类", "当前所有传感器的温度是多少？", "query"),

        // Control Intent
        ("控制类", "把客厅的灯打开", "control"),

        // Creation Intent
        ("创建类", "创建一个规则，当温度超过30度时发送通知", "create"),

        // Multi-Intent (requires multiple actions)
        ("多意图", "列出所有设备，然后告诉我哪些设备在线，最后创建一个告警规则", "multi"),

        // Analysis Intent
        ("分析类", "分析过去24小时的温度数据，找出异常点", "analyze"),

        // Status Inquiry
        ("状态查询", "系统运行状态怎么样？有没有告警？", "status"),
    ];

    let mut metrics = Vec::new();

    for (category, user_input, expected_intent) in test_cases {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("类别: {}", category);
        println!("输入: {}", user_input);
        println!("期望意图: {}", expected_intent);

        let start = Instant::now();

        let input = LlmInput {
            messages: vec![
                Message::system(format!(
                    "/no_think 你是一个智能物联网助手。
分析用户输入的意图类型，返回JSON格式：{{\"intent\": \"意图类型\", \"tools_needed\": [\"需要的工具列表\"]}}

意图类型：
- query: 数据查询
- control: 设备控制
- create: 创建规则/配置
- analyze: 数据分析
- status: 状态查询
- multi: 多种意图组合

请只返回JSON，不要有其他内容。"
                )),
                Message::user(user_input.to_string()),
            ],
            params: GenerationParams {
                max_tokens: Some(8192),
                temperature: Some(0.1), // Low temp for consistent classification
                ..Default::default()
            },
            model: Some("qwen3-vl:2b".to_string()),
            stream: true,
            tools: None,
        };

        match runtime.generate_stream(input).await {
            Ok(mut stream) => {
                let mut full_response = String::new();
                let mut thinking_chars = 0usize;
                let mut content_chars = 0usize;

                while let Some(result) = stream.next().await {
                    match result {
                        Ok((text, is_thinking)) => {
                            if is_thinking {
                                thinking_chars += text.len();
                            } else {
                                content_chars += text.len();
                                full_response.push_str(&text);
                            }
                        }
                        Err(_e) => {
                            break;
                        }
                    }
                }

                let elapsed = start.elapsed();
                println!("响应: {}", full_response.trim());
                println!("用时: {:.2}s", elapsed.as_secs_f64());

                // Parse intent from response
                let intent_correct = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&full_response) {
                    let detected = json.get("intent")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    println!("检测意图: {}", detected);
                    detected == expected_intent || detected.contains(expected_intent)
                } else {
                    // Fallback: check if response contains expected intent
                    full_response.to_lowercase().contains(expected_intent)
                };

                println!("意图识别: {}", if intent_correct { "✅ 正确" } else { "❌ 错误" });

                metrics.push(TestMetrics {
                    tool_calls_count: 0,
                    thinking_chars,
                    content_chars: full_response.len(),
                    execution_time_ms: elapsed.as_millis() as u64,
                    intent_correct,
                    planning_quality: if intent_correct { 1.0 } else { 0.0 },
                });
            }
            Err(e) => {
                println!("❌ 错误: {}", e);
                metrics.push(TestMetrics {
                    tool_calls_count: 0,
                    thinking_chars: 0,
                    content_chars: 0,
                    execution_time_ms: 0,
                    intent_correct: false,
                    planning_quality: 0.0,
                });
            }
        }
        println!();
    }

    // Summary
    let correct_count = metrics.iter().filter(|m| m.intent_correct).count();
    let accuracy = (correct_count as f32 / metrics.len() as f32) * 100.0;
    let avg_time: f64 = metrics.iter().map(|m| m.execution_time_ms as f64).sum::<f64>() / metrics.len() as f64;

    println!("{:=^80}", "");
    println!(" 意图理解测试汇总");
    println!("{:=^80}", "");
    println!("  测试数量: {}", metrics.len());
    println!("  正确数量: {}", correct_count);
    println!("  准确率: {:.1}%", accuracy);
    println!("  平均响应时间: {:.2}ms", avg_time);
    println!("{:=^80}\n", "");

    assert!(accuracy >= 50.0, "Intent understanding accuracy should be at least 50%");
}

/// Test 2: Multi-Tool Calling Performance
/// Tests the agent's ability to call multiple tools in parallel
#[tokio::test]
async fn test_multi_tool_calling() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init();

    println!("\n{:=^80}", "");
    println!(" MULTI-TOOL CALLING PERFORMANCE TEST");
    println!("{:=^80}\n", "");

    let config = OllamaConfig::new("qwen3-vl:2b")
        .with_endpoint("http://localhost:11434");
    let runtime = Arc::new(OllamaRuntime::new(config).expect("Failed to create runtime"));

    let tool_list = vec![
        ("list_devices", "列出所有设备"),
        ("list_rules", "列出所有规则"),
        ("list_device_types", "列出所有设备类型"),
        ("query_data", "查询数据"),
        ("get_status", "获取状态"),
    ];

    let tool_descriptions: Vec<String> = tool_list.iter()
        .map(|(name, desc)| format!("- {}: {}", name, desc))
        .collect();

    let system_prompt = format!(
        "/no_think 你是一个智能物联网助手。

## 可用工具

{}

## 多工具调用规则

如果多个工具之间没有依赖关系，应该在同一个JSON数组中一次性调用，这样可以并行执行。

使用格式：[{{\"name\": \"tool1\", \"arguments\": {{}}}}, {{\"name\": \"tool2\", \"arguments\": {{}}}}]

请只返回工具调用，不要有其他解释文字。",
        tool_descriptions.join("\n")
    );

    let test_cases = vec![
        ("单工具调用", "列出所有设备", 1),
        ("双工具调用（独立）", "同时列出所有设备和所有规则", 2),
        ("三工具调用（独立）", "列出设备、规则和设备类型", 3),
        ("四工具调用（独立）", "列出设备、规则、设备类型，并查询系统状态", 4),
    ];

    let mut metrics = Vec::new();

    for (category, user_input, expected_tools) in test_cases {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("测试: {}", category);
        println!("输入: {}", user_input);
        println!("期望工具数: {}", expected_tools);

        let start = Instant::now();

        let input = LlmInput {
            messages: vec![
                Message::system(system_prompt.clone()),
                Message::user(user_input.to_string()),
            ],
            params: GenerationParams {
                max_tokens: Some(8192),
                temperature: Some(0.3),
                ..Default::default()
            },
            model: Some("qwen3-vl:2b".to_string()),
            stream: true,
            tools: None,
        };

        match runtime.generate_stream(input).await {
            Ok(mut stream) => {
                let mut full_response = String::new();
                let mut thinking_chars = 0usize;
                let mut content_chars = 0usize;

                while let Some(result) = stream.next().await {
                    match result {
                        Ok((text, is_thinking)) => {
                            if is_thinking {
                                thinking_chars += text.len();
                            } else {
                                content_chars += text.len();
                                full_response.push_str(&text);
                            }
                        }
                        Err(_e) => {
                            break;
                        }
                    }
                }

                let elapsed = start.elapsed();

                // Count tool calls in response (JSON format: {"name": "..."} or [{"name": "..."}, ...])
                let tool_call_count = full_response.matches("\"name\":").count();

                println!("响应: {}", full_response.chars().take(200).collect::<String>());
                println!("工具调用数: {}", tool_call_count);
                println!("用时: {:.2}s", elapsed.as_secs_f64());

                let parallel_calling = tool_call_count >= expected_tools.min(2);
                println!("并行调用: {}", if parallel_calling { "✅ 是" } else { "❌ 否" });

                metrics.push(TestMetrics {
                    tool_calls_count: tool_call_count,
                    thinking_chars,
                    content_chars: full_response.len(),
                    execution_time_ms: elapsed.as_millis() as u64,
                    intent_correct: tool_call_count >= expected_tools.saturating_sub(1) as usize,
                    planning_quality: if tool_call_count == expected_tools { 1.0 } else { 0.5 },
                });
            }
            Err(e) => {
                println!("❌ 错误: {}", e);
                metrics.push(TestMetrics {
                    tool_calls_count: 0,
                    thinking_chars: 0,
                    content_chars: 0,
                    execution_time_ms: 0,
                    intent_correct: false,
                    planning_quality: 0.0,
                });
            }
        }
        println!();
    }

    // Summary
    let total_tools: usize = metrics.iter().map(|m| m.tool_calls_count).sum();
    let avg_tools = total_tools as f64 / metrics.len() as f64;
    let avg_time: f64 = metrics.iter().map(|m| m.execution_time_ms as f64).sum::<f64>() / metrics.len() as f64;
    let parallel_count = metrics.iter().filter(|m| m.tool_calls_count > 1).count();

    println!("{:=^80}", "");
    println!(" 多工具调用测试汇总");
    println!("{:=^80}", "");
    println!("  测试数量: {}", metrics.len());
    println!("  总工具调用: {}", total_tools);
    println!("  平均工具数/请求: {:.1}", avg_tools);
    println!("  多工具调用次数: {}", parallel_count);
    println!("  平均响应时间: {:.2}ms", avg_time);
    println!("{:=^80}\n", "");
}

/// Test 3: Task Planning Quality
/// Tests how well the agent plans complex multi-step tasks
#[tokio::test]
async fn test_task_planning() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init();

    println!("\n{:=^80}", "");
    println!(" TASK PLANNING QUALITY TEST");
    println!("{:=^80}\n", "");

    let config = OllamaConfig::new("qwen3-vl:2b")
        .with_endpoint("http://localhost:11434");
    let runtime = Arc::new(OllamaRuntime::new(config).expect("Failed to create runtime"));

    let test_cases = vec![
        (
            "复杂监控场景",
            "我想创建一个智能监控系统：
1. 每分钟检查所有温度传感器
2. 如果温度超过35度，记录警告
3. 如果温度超过40度，发送紧急通知
4. 每小时生成一份报告
5. 当检测到异常时，自动开启风扇降温",
            vec!["query", "create", "control", "schedule"],
            4
        ),
        (
            "设备批量管理",
            "对楼层的所有照明设备进行批量操作：
1. 工作日早上8点自动开灯
2. 工作日晚上6点自动关灯
3. 周末保持关闭状态
4. 手动控制时保持10分钟后恢复自动模式",
            vec!["query", "create", "schedule", "control"],
            3
        ),
        (
            "数据分析与优化",
            "分析能耗数据并优化：
1. 获取过去7天的能耗数据
2. 找出能耗最高的设备
3. 生成可视化报告
4. 根据峰值时段制定节能策略",
            vec!["query", "analyze", "create"],
            3
        ),
    ];

    let mut metrics = Vec::new();

    for (scenario, task, expected_actions, min_steps) in test_cases {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("场景: {}", scenario);
        println!("任务: {}", task);
        println!("期望动作类型: {:?}", expected_actions);
        println!("最小步骤数: {}", min_steps);

        let start = Instant::now();

        let input = LlmInput {
            messages: vec![
                Message::system(format!(
                    "你是一个智能物联网任务规划助手。

分析用户的任务需求，生成执行计划。

返回JSON格式：
{{
  \"analysis\": \"任务分析\",
  \"steps\": [
    {{\"order\": 1, \"action\": \"动作类型\", \"description\": \"步骤描述\", \"tool\": \"使用的工具\"}}
  ],
  \"dependencies\": [\"步骤依赖关系\"],
  \"estimated_time\": \"预计执行时间\"
}}

动作类型包括：
- query: 数据查询
- create: 创建规则/配置
- control: 设备控制
- schedule: 定时任务
- analyze: 数据分析
- notify: 发送通知"
                )),
                Message::user(task.to_string()),
            ],
            params: GenerationParams {
                max_tokens: Some(4096),
                temperature: Some(0.4),
                ..Default::default()
            },
            model: Some("qwen3-vl:2b".to_string()),
            stream: true,
            tools: None,
        };

        match runtime.generate_stream(input).await {
            Ok(mut stream) => {
                let mut full_response = String::new();
                let mut thinking_chars = 0usize;
                let mut content_chars = 0usize;

                while let Some(result) = stream.next().await {
                    match result {
                        Ok((text, is_thinking)) => {
                            if is_thinking {
                                thinking_chars += text.len();
                            } else {
                                content_chars += text.len();
                                full_response.push_str(&text);
                            }
                        }
                        Err(_e) => {
                            break;
                        }
                    }
                }

                let elapsed = start.elapsed();

                // Parse planning quality
                let (step_count, planning_quality) = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&full_response) {
                    let steps = json.get("steps")
                        .and_then(|v: &serde_json::Value| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);

                    println!("计划步骤数: {}", steps);

                    // Check if expected actions are covered
                    let analysis = json.get("analysis")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let actions_covered: usize = expected_actions.iter()
                        .filter(|&action| {
                            full_response.to_lowercase().contains(action) ||
                            analysis.to_lowercase().contains(action)
                        })
                        .count();

                    let quality = (steps as f32 / min_steps as f32).min(1.0) *
                                  (actions_covered as f32 / expected_actions.len() as f32);

                    (steps, quality)
                } else {
                    // Fallback: count indicators in text
                    let step_count = full_response.matches("步骤").count() +
                                   full_response.matches("step").count() +
                                   full_response.matches("1.").count() +
                                   full_response.matches("2.").count();

                    (step_count.max(1), 0.5)
                };

                println!("思考字符: {}", thinking_chars);
                println!("内容字符: {}", content_chars);
                println!("用时: {:.2}s", elapsed.as_secs_f64());
                println!("规划质量: {:.1}%", planning_quality * 100.0);

                metrics.push(TestMetrics {
                    tool_calls_count: step_count,
                    thinking_chars,
                    content_chars,
                    execution_time_ms: elapsed.as_millis() as u64,
                    intent_correct: true,
                    planning_quality,
                });
            }
            Err(e) => {
                println!("❌ 错误: {}", e);
                metrics.push(TestMetrics {
                    tool_calls_count: 0,
                    thinking_chars: 0,
                    content_chars: 0,
                    execution_time_ms: 0,
                    intent_correct: false,
                    planning_quality: 0.0,
                });
            }
        }
        println!();
    }

    // Summary
    let avg_quality: f32 = metrics.iter().map(|m| m.planning_quality).sum::<f32>() / metrics.len() as f32;
    let avg_thinking: f32 = metrics.iter().map(|m| m.thinking_chars as f32).sum::<f32>() / metrics.len() as f32;
    let avg_time: f64 = metrics.iter().map(|m| m.execution_time_ms as f64).sum::<f64>() / metrics.len() as f64;

    println!("{:=^80}", "");
    println!(" 任务规划测试汇总");
    println!("{:=^80}", "");
    println!("  测试场景数: {}", metrics.len());
    println!("  平均规划质量: {:.1}%", avg_quality * 100.0);
    println!("  平均思考字符: {:.0}", avg_thinking);
    println!("  平均规划时间: {:.2}ms", avg_time);
    println!("{:=^80}\n", "");

    assert!(avg_quality >= 0.3, "Average planning quality should be at least 30%");
}

/// Test 4: Complex Conversation with Memory
/// Tests multi-turn conversation with context retention
#[tokio::test]
async fn test_complex_conversation_with_memory() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init();

    println!("\n{:=^80}", "");
    println!(" COMPLEX CONVERSATION WITH MEMORY TEST");
    println!("{:=^80}\n", "");

    let config = OllamaConfig::new("qwen3-vl:2b")
        .with_endpoint("http://localhost:11434");
    let runtime = Arc::new(OllamaRuntime::new(config).expect("Failed to create runtime"));

    // Simulate a complex multi-turn scenario
    let conversation = vec![
        ("Setup", "我有一个智能办公室项目，包含：\n\
            - 10个温度传感器（分布在各个房间）\n\
            - 5个智能开关（控制照明）\n\
            - 2个空气净化器\n\
            - 1个中央空调\n\
            请帮我设计一个自动化系统。"),

        ("Clarification", "温度传感器的测量范围是-20到60度，精度0.5度。\
            智能开关支持开/关/调光（3档）。\
            空气净化器有自动、低速、高速3个模式。"),

        ("Request1", "好的，现在请创建以下规则：\n\
            1. 工作日9点自动开灯\n\
            2. 任何房间温度超过28度时开启空调\n\
            3. 空气质量差时自动开启净化器"),

        ("Followup", "刚才创建的规则中，温度超过28度的规则，我想改成：\
            超过30度时开空调，温度降到26度以下时关空调"),

        ("ComplexQuery", "总结一下当前的配置，包括：\n\
            1. 所有设备清单\n\
            2. 所有自动化规则\n\
            3. 预期的能耗情况\n\
            4. 可能的优化建议"),
    ];

    let mut messages = vec![
        Message::system("你是一个智能物联网系统设计专家，擅长设备管理、规则创建和系统优化。\
        你需要记住之前对话中的所有信息，包括设备配置、规则设置等。".to_string())
    ];

    let mut metrics = Vec::new();

    for (stage, user_input) in conversation {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("阶段: {}", stage);
        println!("用户: {}", user_input);

        messages.push(Message::user(user_input.to_string()));

        let start = Instant::now();

        let input = LlmInput {
            messages: messages.clone(),
            params: GenerationParams {
                max_tokens: Some(8192),
                temperature: Some(0.5),
                ..Default::default()
            },
            model: Some("qwen3-vl:2b".to_string()),
            stream: true,
            tools: None,
        };

        match runtime.generate_stream(input).await {
            Ok(mut stream) => {
                let mut full_response = String::new();
                let mut thinking_chars = 0usize;
                let mut content_chars = 0usize;

                while let Some(result) = stream.next().await {
                    match result {
                        Ok((text, is_thinking)) => {
                            if is_thinking {
                                thinking_chars += text.len();
                            } else {
                                content_chars += text.len();
                                full_response.push_str(&text);
                            }
                        }
                        Err(_e) => {
                            break;
                        }
                    }
                }

                let elapsed = start.elapsed();

                // Truncate for display
                let display = if full_response.chars().count() > 300 {
                    format!("{}...", full_response.chars().take(300).collect::<String>())
                } else {
                    full_response.clone()
                };

                println!("助手: {}", display);
                println!("思考: {} 字符 | 内容: {} 字符 | 用时: {:.2}s",
                    thinking_chars, content_chars, elapsed.as_secs_f64());

                // Add assistant response to history
                messages.push(Message::assistant(&full_response));

                metrics.push(TestMetrics {
                    tool_calls_count: 0,
                    thinking_chars,
                    content_chars,
                    execution_time_ms: elapsed.as_millis() as u64,
                    intent_correct: content_chars > 50,
                    planning_quality: if content_chars > 100 { 1.0 } else { 0.5 },
                });
            }
            Err(e) => {
                println!("❌ 错误: {}", e);
            }
        }
        println!();
    }

    // Summary
    let avg_quality: f32 = metrics.iter().map(|m| m.planning_quality).sum::<f32>() / metrics.len() as f32;
    let avg_time: f64 = metrics.iter().map(|m| m.execution_time_ms as f64).sum::<f64>() / metrics.len() as f64;
    let total_thinking: usize = metrics.iter().map(|m| m.thinking_chars).sum();

    println!("{:=^80}", "");
    println!(" 复杂对话测试汇总");
    println!("{:=^80}", "");
    println!("  对话轮数: {}", metrics.len());
    println!("  平均响应质量: {:.1}%", avg_quality * 100.0);
    println!("  总思考字符: {}", total_thinking);
    println!("  平均响应时间: {:.2}s", avg_time / 1000.0);
    println!("{:=^80}\n", "");
}

/// Test 5: Overall Performance Summary
#[tokio::test]
async fn test_performance_summary() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init();

    println!("\n{:=^80}", "");
    println!(" AGENT PERFORMANCE SUMMARY");
    println!("{:=^80}\n", "");

    let config = OllamaConfig::new("qwen3-vl:2b")
        .with_endpoint("http://localhost:11434");
    let runtime = Arc::new(OllamaRuntime::new(config).expect("Failed to create runtime"));

    println!("正在运行性能基准测试...\n");

    let tests = vec![
        ("简单问答", "你好，请介绍一下自己", 512),
        ("工具调用准备", "列出所有可用的工具", 1024),
        ("规则查询", "查看当前的自动化规则", 1024),
        ("任务规划", "帮我规划一个智能家居系统", 2048),
    ];

    let mut results = std::collections::HashMap::new();

    for (name, prompt, expected_tokens) in tests {
        let start = Instant::now();

        let input = LlmInput {
            messages: vec![
                Message::system("你是一个智能物联网助手。".to_string()),
                Message::user(prompt.to_string()),
            ],
            params: GenerationParams {
                max_tokens: Some(expected_tokens),
                temperature: Some(0.5),
                ..Default::default()
            },
            model: Some("qwen3-vl:2b".to_string()),
            stream: true,
            tools: None,
        };

        match runtime.generate_stream(input).await {
            Ok(mut stream) => {
                let mut chars = 0usize;
                while let Some(result) = stream.next().await {
                    if let Ok((text, _)) = result {
                        chars += text.len();
                    }
                }
                let elapsed = start.elapsed();
                results.insert(name, (chars, elapsed.as_secs_f64()));
            }
            Err(_) => {}
        }
    }

    println!("{:=^80}", "");
    println!(" 性能基准结果");
    println!("{:=^80}", "");
    for (name, (chars, time)) in &results {
        println!("  {:20} | 字符: {:6} | 用时: {:.2}s | 速度: {:.0} 字符/秒",
            name, chars, time, *chars as f64 / time);
    }
    println!("{:=^80}\n", "");
}
