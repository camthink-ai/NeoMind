//! AI Agent with Real LLM Backend Integration Test
//!
//! This test requires Ollama to be running on localhost:11434
//! with at least one model available (e.g., qwen2.5:3b)

use std::sync::Arc;
use edge_ai_core::EventBus;
use edge_ai_storage::{
    AgentStore, AgentSchedule, AgentStats, AgentStatus, AiAgent, AgentMemory,
    ScheduleType, WorkingMemory, ShortTermMemory, LongTermMemory,
};
use edge_ai_agent::ai_agent::{AgentExecutor, AgentExecutorConfig};
use edge_ai_llm::backends::ollama::{OllamaRuntime, OllamaConfig};
use edge_ai_core::llm::backend::LlmRuntime;

/// Test context with real LLM backend
struct LlmTestContext {
    pub store: Arc<AgentStore>,
    pub executor: AgentExecutor,
    pub event_bus: Arc<EventBus>,
    pub llm_runtime: Arc<OllamaRuntime>,
}

impl LlmTestContext {
    async fn new() -> anyhow::Result<Self> {
        // Use memory store for testing
        let store = AgentStore::memory()?;
        let event_bus = Arc::new(EventBus::new());

        // Create real LLM backend
        let ollama_config = OllamaConfig {
            endpoint: "http://localhost:11434".to_string(),
            model: "qwen2.5:3b".to_string(),
            timeout_secs: 120,
        };

        let llm_runtime = Arc::new(OllamaRuntime::new(ollama_config)?);

        let executor_config = AgentExecutorConfig {
            store: store.clone(),
            time_series_storage: None,
            device_service: None,
            event_bus: Some(event_bus.clone()),
            message_manager: None,
            llm_runtime: Some(llm_runtime.clone()),
            llm_backend_store: None,
        };

        let executor = AgentExecutor::new(executor_config).await?;

        Ok(Self {
            store,
            executor,
            event_bus,
            llm_runtime,
        })
    }

    async fn create_test_agent(
        &self,
        name: &str,
        user_prompt: &str,
    ) -> anyhow::Result<AiAgent> {
        let now = chrono::Utc::now().timestamp();

        let agent = AiAgent {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: None,
            user_prompt: user_prompt.to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(60),
                cron_expression: None,
                timezone: None,
                event_filter: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: now,
            updated_at: now,
            last_execution_at: None,
            stats: AgentStats {
                total_executions: 0,
                successful_executions: 0,
                failed_executions: 0,
                avg_duration_ms: 0,
                last_duration_ms: Some(0),
            },
            memory: AgentMemory {
                working: WorkingMemory::default(),
                short_term: ShortTermMemory::default(),
                long_term: LongTermMemory::default(),
                state_variables: Default::default(),
                baselines: Default::default(),
                learned_patterns: vec![],
                trend_data: vec![],
                updated_at: now,
            },
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 5,
            error_message: None,
        };

        self.store.save_agent(&agent).await?;
        Ok(agent)
    }
}

// ========== Helper to check if Ollama is available ==========

fn ollama_available() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 11434));
    match std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)) {
        Ok(_) => true,
        Err(_) => false,
    }
}

// ========== Tests ==========

#[tokio::test]
#[ignore = "Requires Ollama to be running. Run with: cargo test --test llm_integration_test -- --ignored"]
async fn test_llm_monitor_agent() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }

    let ctx = LlmTestContext::new().await?;

    println!("\n=== 测试 Monitor Agent with Real LLM ===");

    let agent = ctx.create_test_agent(
        "温度监控Agent",
        "监控温度传感器，当温度超过30度时发出告警",
    ).await?;

    let agent_id = agent.id.clone();
    println!("创建 Agent: {}", agent.name);

    // Execute the agent - this will use real LLM
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    let record = ctx.executor.execute_agent(agent.clone()).await?;

    println!("\n执行结果:");
    println!("  状态: {:?}", record.status);
    println!("  时长: {}ms", record.duration_ms);
    println!("  决策过程:");

    // Show the actual LLM response
    println!("    情况分析: {}", record.decision_process.situation_analysis);
    println!("    推理步骤数: {}", record.decision_process.reasoning_steps.len());
    for (i, step) in record.decision_process.reasoning_steps.iter().enumerate() {
        println!("      步骤{}: {}", i + 1, step.description);
    }
    println!("    决策数: {}", record.decision_process.decisions.len());
    println!("    结论: {}", record.decision_process.conclusion);

    // Verify conversation history was created
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    println!("\n对话历史长度: {}", agent.conversation_history.len());
    assert_eq!(agent.conversation_history.len(), 1);

    let turn = &agent.conversation_history[0];
    println!("  执行ID: {}", turn.execution_id);
    println!("  成功: {}", turn.success);

    assert!(turn.success, "Execution should succeed");
    assert!(!record.decision_process.situation_analysis.is_empty(),
        "LLM should provide analysis");

    println!("\n✅ Monitor Agent 测试通过！");
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama to be running. Run with: cargo test --test llm_integration_test -- --ignored"]
async fn test_llm_executor_agent() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }

    let ctx = LlmTestContext::new().await?;

    println!("\n=== 测试 Executor Agent with Real LLM ===");

    let agent = ctx.create_test_agent(
        "开关控制Agent",
        "当温度超过25度时，打开风扇开关。当温度低于20度时，关闭风扇开关",
    ).await?;

    let agent_id = agent.id.clone();
    println!("创建 Agent: {}", agent.name);

    // Execute the agent
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    let record = ctx.executor.execute_agent(agent.clone()).await?;

    println!("\n执行结果:");
    println!("  状态: {:?}", record.status);
    println!("  时长: {}ms", record.duration_ms);
    println!("  决策数: {}", record.decision_process.decisions.len());

    for (i, decision) in record.decision_process.decisions.iter().enumerate() {
        println!("    决策{}: {} - {}", i + 1, decision.decision_type, decision.description);
    }

    println!("  结论: {}", record.decision_process.conclusion);

    // Verify conversation history
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    assert_eq!(agent.conversation_history.len(), 1);

    println!("\n✅ Executor Agent 测试通过！");
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama to be running. Run with: cargo test --test llm_integration_test -- --ignored"]
async fn test_llm_analyst_agent() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }

    let ctx = LlmTestContext::new().await?;

    println!("\n=== 测试 Analyst Agent with Real LLM ===");

    let agent = ctx.create_test_agent(
        "数据分析Agent",
        "分析温度数据趋势，识别异常模式，生成周报",
    ).await?;

    let agent_id = agent.id.clone();
    println!("创建 Agent: {}", agent.name);

    // Execute the agent
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    let record = ctx.executor.execute_agent(agent.clone()).await?;

    println!("\n执行结果:");
    println!("  状态: {:?}", record.status);
    println!("  时长: {}ms", record.duration_ms);
    println!("  情况分析:");
    println!("    {}", record.decision_process.situation_analysis);
    println!("  推理步骤数: {}", record.decision_process.reasoning_steps.len());
    println!("  结论:");
    println!("    {}", record.decision_process.conclusion);

    // Verify conversation history
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    assert_eq!(agent.conversation_history.len(), 1);

    println!("\n✅ Analyst Agent 测试通过！");
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama to be running. Run with: cargo test --test llm_integration_test -- --ignored"]
async fn test_llm_conversation_context() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }

    let ctx = LlmTestContext::new().await?;

    println!("\n=== 测试对话上下文累积 ===");

    let agent = ctx.create_test_agent(
        "上下文测试Agent",
        "监控传感器数据，记住之前的读数，检测趋势变化",
    ).await?;

    let agent_id = agent.id.clone();
    println!("创建 Agent: {}", agent.name);

    // Execute multiple times - each execution should have access to previous context
    println!("\n--- 第1次执行 ---");
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    let record1 = ctx.executor.execute_agent(agent.clone()).await?;
    println!("  分析: {}", record1.decision_process.situation_analysis);
    println!("  时长: {}ms", record1.duration_ms);

    println!("\n--- 第2次执行 ---");
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    let record2 = ctx.executor.execute_agent(agent.clone()).await?;
    println!("  分析: {}", record2.decision_process.situation_analysis);
    println!("  时长: {}ms", record2.duration_ms);

    println!("\n--- 第3次执行 ---");
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    let record3 = ctx.executor.execute_agent(agent.clone()).await?;
    println!("  分析: {}", record3.decision_process.situation_analysis);
    println!("  时长: {}ms", record3.duration_ms);

    // Verify conversation history accumulated
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    println!("\n对话历史长度: {}", agent.conversation_history.len());
    assert_eq!(agent.conversation_history.len(), 3);

    // Show the conversation turns
    for (i, turn) in agent.conversation_history.iter().enumerate() {
        println!("  轮次{}: 触发={}, 成功={}, 时长={}ms",
            i + 1, turn.trigger_type, turn.success, turn.duration_ms);
    }

    println!("\n✅ 对话上下文测试通过！");
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama to be running. Run with: cargo test --test llm_integration_test -- --ignored"]
async fn test_llm_direct_api() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }

    println!("\n=== 测试直接 LLM API 调用 ===");

    let ollama_config = OllamaConfig {
        endpoint: "http://localhost:11434".to_string(),
        model: "qwen2.5:3b".to_string(),
        timeout_secs: 60,
    };

    let llm_runtime = OllamaRuntime::new(ollama_config)?;

    use edge_ai_core::llm::backend::{LlmInput, GenerationParams};
    use edge_ai_core::message::Message;

    let messages = vec![
        Message::system("你是一个物联网设备监控助手。"),
        Message::user("请用一句话说明监控专员的主要职责。"),
    ];

    let input = LlmInput {
        messages,
        params: GenerationParams {
            temperature: Some(0.7),
            max_tokens: Some(200),
            ..Default::default()
        },
        model: Some("qwen2.5:3b".to_string()),
        stream: false,
        tools: None,
    };

    println!("发送请求到 LLM...");
    let start = std::time::Instant::now();
    let output = llm_runtime.generate(input).await?;
    let elapsed = start.elapsed();

    println!("\nLLM 响应:");
    println!("  内容: {}", output.text);
    println!("  完成原因: {:?}", output.finish_reason);
    println!("  Token 使用: {:?}", output.usage);
    println!("  响应时间: {:?}", elapsed);

    assert!(!output.text.is_empty(), "LLM should return non-empty response");
    assert!(elapsed.as_secs() < 30, "Response should be reasonably fast");

    println!("\n✅ 直接 LLM API 测试通过！");
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama to be running. Run with: cargo test --test llm_integration_test -- --ignored"]
async fn test_llm_agent_comparison() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama not available, skipping test");
        return Ok(());
    }

    let ctx = LlmTestContext::new().await?;

    println!("\n=== 对比测试不同类型的 Agents ===");

    let test_cases = vec![
        ("温度监控", "监控温度传感器，超过30度告警"),
        ("开关控制", "高温时打开风扇"),
        ("趋势分析", "分析温度数据趋势"),
    ];

    let mut results = Vec::new();

    for (name, prompt) in test_cases {
        println!("\n--- 测试: {} ---", name);

        let agent = ctx.create_test_agent(name, prompt).await?;

        let agent = ctx.store.get_agent(&agent.id).await?.unwrap();
        let record = ctx.executor.execute_agent(agent.clone()).await?;

        println!("  状态: {:?}", record.status);
        println!("  分析: {}", record.decision_process.situation_analysis);
        println!("  结论: {}", record.decision_process.conclusion);
        println!("  耗时: {}ms", record.duration_ms);

        results.push((name, record.duration_ms));
    }

    println!("\n性能汇总:");
    for (name, duration) in &results {
        println!("  {}: {}ms", name, duration);
    }

    let avg_duration: u64 = results.iter().map(|(_, d)| d).sum::<u64>() / results.len() as u64;
    println!("  平均: {}ms", avg_duration);

    assert!(avg_duration < 30000, "Average execution time should be under 30 seconds");

    println!("\n✅ Agent 对比测试通过！");
    Ok(())
}
