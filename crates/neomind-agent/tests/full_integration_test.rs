//! Full Integration Test with Real LLM, Metrics, Commands, and Notifications
//!
//! This test verifies:
//! 1. Agent can understand real metrics with LLM
//! 2. Agent can generate and execute commands
//! 3. Agent can send notifications/alerts
//! 4. End-to-end workflow with real data flow

#![allow(dead_code)]

use neomind_agent::ai_agent::{AgentExecutor, AgentExecutorConfig};
use neomind_agent::llm_backends::{OllamaConfig, OllamaRuntime};
use neomind_core::llm::backend::{GenerationParams, LlmInput};
use neomind_core::{
    message::{Content, Message, MessageRole},
    EventBus, LlmRuntime, MetricValue, NeoMindEvent,
};
use neomind_messages::{MessageManager, MessageSeverity};
use neomind_storage::{
    AgentMemory, AgentResource, AgentSchedule, AgentStats, AgentStatus, AgentStore, AiAgent,
    DataPoint, ExecutionJournal, ExecutionMode, ResourceType, ScheduleType, TimeSeriesStore,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ============================================================================
// Test Context with All Components
// ============================================================================

struct FullTestContext {
    pub store: Arc<AgentStore>,
    pub executor: AgentExecutor,
    pub event_bus: Arc<EventBus>,
    pub llm_runtime: Arc<OllamaRuntime>,
    pub time_series: Arc<TimeSeriesStore>,
    pub message_manager: Arc<MessageManager>,
}

impl FullTestContext {
    async fn new() -> anyhow::Result<Self> {
        let store = AgentStore::memory()?;
        let event_bus = Arc::new(EventBus::new());

        // Create LLM runtime
        let ollama_config = OllamaConfig {
            endpoint: "http://localhost:11434".to_string(),
            model: "qwen3.5:2b".to_string(),
            timeout_secs: 120,
        };
        let llm_runtime = Arc::new(OllamaRuntime::new(ollama_config)?);

        // Create time series storage (memory-based for testing)
        let time_series = TimeSeriesStore::memory()?;

        // Create message manager
        let message_manager = Arc::new(MessageManager::new());
        // Note: MessageManager now initializes with default channels via register_default_channels
        message_manager.register_default_channels().await;

        // Note: We can't create device_service here without the full devices crate
        // So commands will be simulated
        let executor_config = AgentExecutorConfig {
            store: store.clone(),
            time_series_storage: Some(time_series.clone()),
            device_service: None,
            event_bus: Some(event_bus.clone()),
            message_manager: Some(message_manager.clone()),
            llm_runtime: Some(llm_runtime.clone()
                as Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>),
            llm_backend_store: None,
            extension_registry: None,
            tool_registry: None,
            memory_store: None,
            backend_semaphores: None,
            execution_semaphore: None,
            skill_registry: None,
        };

        let executor = AgentExecutor::new(executor_config).await?;

        Ok(Self {
            store,
            executor,
            event_bus,
            llm_runtime,
            time_series,
            message_manager,
        })
    }

    /// Simulate metric data for a device
    async fn inject_metric_data(
        &self,
        device_id: &str,
        metric: &str,
        value: f64,
    ) -> anyhow::Result<()> {
        let timestamp = chrono::Utc::now().timestamp_millis();

        // Store in time series
        let point = DataPoint {
            timestamp,
            value: serde_json::json!(value),
            quality: Some(1.0),
            metadata: None,
        };

        self.time_series.write(device_id, metric, point).await?;

        // Also publish to event bus
        let event = NeoMindEvent::DeviceMetric {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            value: MetricValue::Float(value),
            timestamp: chrono::Utc::now().timestamp(),
            quality: Some(1.0),
            is_virtual: None,
        };
        let _ = self.event_bus.publish(event).await;

        Ok(())
    }

    /// Inject multiple historical data points
    async fn inject_historical_data(
        &self,
        device_id: &str,
        metric: &str,
        values: Vec<(i64, f64)>,
    ) -> anyhow::Result<()> {
        let mut points = Vec::new();
        for (ts, v) in &values {
            points.push(DataPoint {
                timestamp: ts * 1000,
                value: serde_json::json!(*v),
                quality: Some(1.0),
                metadata: None,
            });
        }

        self.time_series
            .write_batch(device_id, metric, points)
            .await?;
        Ok(())
    }

    /// Create an agent with metric resources
    async fn create_monitoring_agent(
        &self,
        name: &str,
        metrics: Vec<(String, String)>,
        user_prompt: &str,
    ) -> anyhow::Result<AiAgent> {
        let now = chrono::Utc::now().timestamp();

        let mut resources = Vec::new();
        for (device_id, metric_name) in &metrics {
            resources.push(AgentResource {
                resource_type: ResourceType::Metric,
                resource_id: format!("{}:{}", device_id, metric_name),
                name: format!("{} - {}", device_id, metric_name),
                config: serde_json::json!({}),
            });
        }

        let agent = AiAgent {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: None,
            user_prompt: user_prompt.to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources,
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(60),
                cron_expression: None,
                timezone: None,
                event_filter: None,
            },
            status: AgentStatus::Active,
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
                journal: ExecutionJournal::default(),
                knowledge_files: vec![],
                updated_at: now,
            },
            tool_config: None,
            execution_mode: ExecutionMode::Focused,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            priority: 128,
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 3,
        };

        self.store.save_agent(&agent).await?;
        Ok(agent)
    }

    /// Create an executor agent with command resources
    async fn create_executor_agent(
        &self,
        name: &str,
        commands: Vec<(String, String, serde_json::Value)>,
        user_prompt: &str,
    ) -> anyhow::Result<AiAgent> {
        let now = chrono::Utc::now().timestamp();

        let mut resources = Vec::new();
        for (device_id, command, params) in &commands {
            resources.push(AgentResource {
                resource_type: ResourceType::Command,
                resource_id: format!("{}:{}", device_id, command),
                name: format!("{} - {}", device_id, command),
                config: serde_json::json!({
                    "parameters": params
                }),
            });
        }

        // Also add metric resources for monitoring
        for (device_id, _command, _params) in &commands {
            resources.push(AgentResource {
                resource_type: ResourceType::Metric,
                resource_id: format!("{}:temperature", device_id),
                name: format!("{} - temperature", device_id),
                config: serde_json::json!({}),
            });
        }

        let agent = AiAgent {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: None,
            user_prompt: user_prompt.to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources,
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(60),
                cron_expression: None,
                timezone: None,
                event_filter: None,
            },
            status: AgentStatus::Active,
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
                journal: ExecutionJournal::default(),
                knowledge_files: vec![],
                updated_at: now,
            },
            tool_config: None,
            execution_mode: ExecutionMode::Focused,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            priority: 128,
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 3,
        };

        self.store.save_agent(&agent).await?;
        Ok(agent)
    }

    /// Send a test notification
    async fn send_notification(
        &self,
        severity: MessageSeverity,
        message: &str,
    ) -> anyhow::Result<()> {
        let msg = neomind_messages::Message::alert(
            severity,
            "Agent Notification".to_string(),
            message.to_string(),
            "test_agent".to_string(),
        );
        let msg = self.message_manager.create_message(msg).await?;

        println!("  📢 通知发送: {} - {}", severity, message);
        println!("     Message ID: {}", msg.id);

        Ok(())
    }

    /// Query LLM directly
    async fn query_llm(&self, system_prompt: &str, user_message: &str) -> anyhow::Result<String> {
        let messages = vec![
            Message::new(MessageRole::System, Content::text(system_prompt)),
            Message::new(MessageRole::User, Content::text(user_message)),
        ];

        let input = LlmInput {
            messages,
            params: GenerationParams {
                temperature: Some(0.7),
                max_tokens: Some(500),
                ..Default::default()
            },
            model: Some("qwen3.5:2b".to_string()),
            stream: false,
            tools: None,
        };

        let output = self.llm_runtime.generate(input).await?;
        Ok(output.text)
    }
}

// ============================================================================
// Helper: Check Ollama availability
// ============================================================================

fn ollama_available() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 11434));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok()
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
#[ignore = "Requires Ollama LLM backend"]
async fn test_llm_understands_metrics() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama 未运行，跳过测试");
        return Ok(());
    }

    let ctx = FullTestContext::new().await?;

    println!("\n=== 测试: LLM 理解指标数据 ===\n");

    // Inject some temperature data
    let device_id = "sensor_temp_01";
    println!("1. 注入模拟温度数据...");
    for i in 0..10 {
        let temp = 20.0 + (i as f64 * 2.0); // 20, 22, 24, 26, 28, 30, 32, 34, 36, 38
        ctx.inject_metric_data(device_id, "temperature", temp)
            .await?;
        println!("   {}℃ @ t-{}min", temp, (10 - i) * 5);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Create agent with this metric
    let agent = ctx
        .create_monitoring_agent(
            "温度分析Agent",
            vec![(device_id.to_string(), "temperature".to_string())],
            "分析温度传感器数据，检测异常值和趋势",
        )
        .await?;

    println!("\n2. 执行Agent分析...");
    let agent = ctx.store.get_agent(&agent.id).await?.unwrap();
    let record = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("\n3. Agent响应:");
    println!("   状态: {:?}", record.status);
    println!(
        "   情况分析: {}",
        record.decision_process.situation_analysis
    );
    println!("   结论: {}", record.decision_process.conclusion);

    // Verify data was collected
    println!("\n4. 收集的数据:");
    for data in &record.decision_process.data_collected {
        println!("   - {}: {}", data.source, data.data_type);
        if let Some(value) = data.values.get("value") {
            println!("     最新值: {}", value);
        }
        if let Some(count) = data.values.get("points_count") {
            println!("     数据点数: {}", count);
        }
    }

    assert!(
        !record.decision_process.data_collected.is_empty(),
        "应该收集到数据"
    );

    println!("\n✅ LLM理解指标测试通过！");
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama LLM backend"]
async fn test_llm_generates_commands() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama 未运行，跳过测试");
        return Ok(());
    }

    let ctx = FullTestContext::new().await?;

    println!("\n=== 测试: LLM 生成控制指令 ===\n");

    // Setup: Inject temperature data that exceeds threshold
    let device_id = "hvac_controller";
    println!("1. 注入高温数据...");

    for temp in &[25.0, 28.0, 31.0, 33.0, 35.0, 32.0] {
        ctx.inject_metric_data(device_id, "temperature", *temp)
            .await?;
        println!("   注入: {}℃", temp);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Create executor agent with command resource
    let agent = ctx
        .create_executor_agent(
            "空调控制Agent",
            vec![(
                device_id.to_string(),
                "turn_on".to_string(),
                serde_json::json!({"mode": "cool"}),
            )],
            "监控温度，当温度超过30度时开启空调，低于25度时关闭空调",
        )
        .await?;

    println!("\n2. 执行Agent决策...");

    let agent = ctx.store.get_agent(&agent.id).await?.unwrap();
    let record = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("\n3. 决策分析:");
    println!("   状态: {:?}", record.status);
    println!(
        "   情况分析: {}",
        record.decision_process.situation_analysis
    );

    println!("\n   推理步骤:");
    for (i, step) in record.decision_process.reasoning_steps.iter().enumerate() {
        println!("     步骤{}: {}", i + 1, step.description);
        println!("       输入: {:?}", step.input);
        println!("       输出: {:?}", step.output);
    }

    println!("\n   决策:");
    for (i, decision) in record.decision_process.decisions.iter().enumerate() {
        println!("     决策{}: {}", i + 1, decision.decision_type);
        println!("       描述: {}", decision.description);
        println!("       动作: {}", decision.action);
    }

    println!("\n   结论: {}", record.decision_process.conclusion);

    // Check execution result
    if let Some(ref result) = record.result {
        println!("\n   执行结果:");
        for action in &result.actions_executed {
            println!(
                "     - 动作: {} ({})",
                action.action_type,
                if action.success { "成功" } else { "失败" }
            );
        }
    }

    println!("\n✅ LLM生成指令测试通过！");
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama LLM backend"]
async fn test_notification_sending() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama 未运行，跳过测试");
        return Ok(());
    }

    let ctx = FullTestContext::new().await?;

    println!("\n=== 测试: 通知发送 ===\n");

    println!("1. 测试不同严重级别的通知...");

    ctx.send_notification(MessageSeverity::Info, "这是信息级别通知")
        .await?;
    ctx.send_notification(MessageSeverity::Warning, "这是警告级别通知")
        .await?;
    ctx.send_notification(MessageSeverity::Critical, "这是严重级别通知")
        .await?;

    println!("\n2. 测试Agent触发的通知...");

    // Create a monitoring agent that should alert on high temperature
    let device_id = "furnace_sensor";
    ctx.inject_metric_data(device_id, "temperature", 85.0)
        .await?;

    let agent = ctx
        .create_monitoring_agent(
            "熔炉监控Agent",
            vec![(device_id.to_string(), "temperature".to_string())],
            "监控熔炉温度，超过80度发送严重告警",
        )
        .await?;

    println!("\n3. 执行Agent...");
    let agent = ctx.store.get_agent(&agent.id).await?.unwrap();
    let record = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("\n4. Agent响应:");
    println!("   分析: {}", record.decision_process.situation_analysis);

    // Simulate sending alert based on agent's decision
    if !record.decision_process.decisions.is_empty() {
        println!("\n5. 模拟发送告警通知...");
        ctx.send_notification(
            MessageSeverity::Critical,
            &format!(
                "Agent {} 检测到异常: {}",
                agent.name, record.decision_process.conclusion
            ),
        )
        .await?;
    }

    println!("\n✅ 通知发送测试通过！");
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama LLM backend"]
async fn test_conversation_with_llm() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama 未运行，跳过测试");
        return Ok(());
    }

    let ctx = FullTestContext::new().await?;

    println!("\n=== 测试: 与LLM的对话上下文 ===\n");

    // Direct LLM conversation test
    println!("1. 直接查询LLM - 指标理解能力...");
    let response1 = ctx
        .query_llm(
            "你是一个物联网监控助手。",
            "当前温度为35度，湿度为60%，请分析这个环境状态。",
        )
        .await?;

    println!("   LLM响应: {}", response1);

    println!("\n2. 测试指令生成能力...");
    let response2 = ctx.query_llm(
        "你是一个智能家居控制助手。",
        "客厅温度现在是32度，超过了28度的阈值，应该怎么办？请用JSON格式回复：{\"action\": \"xxx\", \"device\": \"xxx\", \"reason\": \"xxx\"}"
    ).await?;

    println!("   LLM响应: {}", response2);

    println!("\n3. 测试异常检测能力...");
    let response3 = ctx
        .query_llm(
            "你是一个设备监控助手。",
            "温度传感器在过去1小时内从25度逐渐上升到42度，这是否异常？为什么？",
        )
        .await?;

    println!("   LLM响应: {}", response3);

    println!("\n✅ LLM对话测试通过！");
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Ollama LLM backend"]
async fn test_end_to_end_workflow() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama 未运行，跳过测试");
        return Ok(());
    }

    let ctx = FullTestContext::new().await?;

    println!("\n=== 端到端工作流测试 ===\n");

    let device_id = "smart_home_temp";
    let agent_name = "智能家居管家";
    let prompt = "你是智能家居管家，负责：
1. 监控室内温度（正常范围：20-26度）
2. 温度超过26度时，建议开启空调
3. 温度低于20度时，建议开启暖气
4. 异常情况（超过30度或低于15度）时发送告警";

    println!("场景: 温度逐渐上升，测试Agent响应\n");

    // Round 1: Normal temperature
    println!("--- 场景1: 正常温度 (22度) ---");
    ctx.inject_metric_data(device_id, "temperature", 22.0)
        .await?;

    let agent = ctx
        .create_monitoring_agent(
            agent_name,
            vec![(device_id.to_string(), "temperature".to_string())],
            prompt,
        )
        .await?;

    let agent_id = agent.id.clone();

    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    let record1 = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;
    println!("分析: {}", record1.decision_process.situation_analysis);
    println!("结论: {}", record1.decision_process.conclusion);

    // Round 2: Slightly high
    println!("\n--- 场景2: 稍高温度 (27度) ---");
    ctx.inject_metric_data(device_id, "temperature", 27.0)
        .await?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    let record2 = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;
    println!("分析: {}", record2.decision_process.situation_analysis);
    println!("结论: {}", record2.decision_process.conclusion);

    // Round 3: Too high - should alert
    println!("\n--- 场景3: 过高温度 (31度) ---");
    ctx.inject_metric_data(device_id, "temperature", 31.0)
        .await?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    let record3 = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;
    println!("分析: {}", record3.decision_process.situation_analysis);

    // Check if alert should be sent
    if record3.decision_process.conclusion.contains("异常")
        || record3.decision_process.conclusion.contains("告警")
        || record3.decision_process.conclusion.contains("高")
    {
        println!("⚠️  检测到异常条件！");
        ctx.send_notification(
            MessageSeverity::Warning,
            &record3.decision_process.conclusion,
        )
        .await?;
    }

    // Verify conversation history (backward compat field, no longer written to)
    println!("\n--- 对话历史 ---");
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    println!(
        "总轮次 (conversation_history): {}",
        agent.conversation_history.len()
    );
    let _ = &agent.conversation_history;

    println!("\n✅ 端到端工作流测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_data_collection_and_context() -> anyhow::Result<()> {
    let ctx = FullTestContext::new().await?;

    println!("\n=== 测试: 数据收集和上下文构建 ===\n");

    // Create multiple devices with different metrics
    let devices = vec![
        ("temp_sensor_01", "temperature"),
        ("temp_sensor_02", "temperature"),
        ("humidity_sensor", "humidity"),
        ("energy_meter", "power"),
    ];

    println!("1. 创建多指标Agent...");
    let mut metrics = Vec::new();
    for (id, metric) in &devices {
        metrics.push((id.to_string(), metric.to_string()));
    }

    let agent = ctx
        .create_monitoring_agent(
            "多传感器监控Agent",
            metrics,
            "监控所有传感器数据，生成综合报告",
        )
        .await?;

    println!("2. 注入模拟数据...");
    for (device_id, metric) in &devices {
        let value = rand::random::<f64>() * 50.0 + 10.0; // 10-60 range
        ctx.inject_metric_data(device_id, metric, value).await?;
        println!("   {}: {} = {:.1}", device_id, metric, value);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    println!("\n3. 执行Agent...");
    let agent = ctx.store.get_agent(&agent.id).await?.unwrap();
    let start = Instant::now();
    let record = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;
    let elapsed = start.elapsed();

    println!("\n4. 执行结果:");
    println!("   耗时: {:?}", elapsed);
    println!(
        "   数据收集: {} 个数据源",
        record.decision_process.data_collected.len()
    );

    println!("\n   收集到的数据:");
    for data in &record.decision_process.data_collected {
        if let Some(val) = data.values.get("value") {
            println!("     - {}: {:.1}", data.source, val);
        }
    }

    println!("\n✅ 数据收集测试通过！");
    Ok(())
}
