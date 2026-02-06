//! AI Agent Conversation History Integration Test

use std::sync::Arc;
use neomind_core::{EventBus, message::{Message, MessageRole, Content}, MetricValue, NeoMindEvent};
use neomind_storage::{
    AgentStore, AgentSchedule, AgentResource, AgentStats, AgentStatus, AiAgent, AgentMemory,
    WorkingMemory, ShortTermMemory, LongTermMemory,
    DataCollected, DecisionProcess, Decision, ReasoningStep,
    ConversationTurn, TurnInput, TurnOutput, ScheduleType,
};
use neomind_agent::ai_agent::{AgentExecutor, AgentExecutorConfig};

/// Test context
struct TestContext {
    pub store: Arc<AgentStore>,
    pub executor: AgentExecutor,
    pub event_bus: Arc<EventBus>,
}

impl TestContext {
    async fn new() -> anyhow::Result<Self> {
        // Use memory store for testing
        let store = AgentStore::memory()?;
        let event_bus = Arc::new(EventBus::new());

        let executor_config = AgentExecutorConfig {
            store: store.clone(),
            time_series_storage: None,
            device_service: None,
            event_bus: Some(event_bus.clone()),
            message_manager: None,
            llm_runtime: None,
            llm_backend_store: None,
        };

        let executor = AgentExecutor::new(executor_config).await?;

        Ok(Self {
            store,
            executor,
            event_bus,
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
                state_variables: Default::default(),
                baselines: Default::default(),
                learned_patterns: vec![],
                trend_data: vec![],
                updated_at: now,
                working: WorkingMemory::default(),
                short_term: ShortTermMemory::default(),
                long_term: LongTermMemory::default(),
            },
            error_message: None,
            priority: 128,
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
        };

        self.store.save_agent(&agent).await?;
        Ok(agent)
    }

    async fn load_agent(&self, agent_id: &str) -> anyhow::Result<AiAgent> {
        Ok(self.store.get_agent(agent_id).await?
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?)
    }

    async fn get_conversation_history(&self, agent_id: &str) -> Vec<ConversationTurn> {
        self.store.get_conversation_history(agent_id, None).await
            .unwrap_or_default()
    }
}

// ========== Tests ==========

#[tokio::test]
async fn test_conversation_history_basic() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试基础对话历史 ===");

    let agent = ctx.create_test_agent(
        "测试Agent",
        "监控传感器数据",
    ).await?;

    let agent_id = agent.id.clone();
    println!("创建 Agent: {}", agent.name);

    // Execute 3 times
    for i in 0..3 {
        println!("\n--- 执行 #{} ---", i + 1);

        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let record = ctx.executor.execute_agent(agent.clone()).await?;

        println!("状态: {:?}", record.status);
        println!("时长: {}ms", record.duration_ms);

        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        println!("对话历史长度: {}", agent.conversation_history.len());

        assert_eq!(agent.conversation_history.len(), i + 1);
    }

    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    assert_eq!(agent.conversation_history.len(), 3);

    // Verify each turn
    for (i, turn) in agent.conversation_history.iter().enumerate() {
        println!("\n轮次 #{}:", i + 1);
        println!("  执行ID: {}", turn.execution_id);
        println!("  触发: {}", turn.trigger_type);
        println!("  成功: {}", turn.success);
    }

    println!("\n✅ 基础对话历史测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_all_agent_roles() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试所有 Agent 类型 ===");

    let agent_configs = vec![
        ("监控专员", "监控传感器数据，检测异常"),
        ("执行专员", "执行控制指令，操作设备"),
        ("分析专员", "分析数据趋势，生成报告"),
    ];

    for (name, prompt) in &agent_configs {
        println!("\n--- 测试类型: {} ---", name);

        let agent = ctx.create_test_agent(
            &format!("{}_test", name),
            prompt,
        ).await?;

        println!("创建: {}", agent.name);

        // Execute once
        let record = ctx.executor.execute_agent(agent.clone()).await?;
        println!("执行状态: {:?}", record.status);

        // Reload and check history
        let agent = ctx.store.get_agent(&agent.id).await?.unwrap();
        assert_eq!(agent.conversation_history.len(), 1);
    }

    println!("\n✅ 所有类型测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_conversation_persistence() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试对话持久化 ===");

    let agent = ctx.create_test_agent(
        "持久化测试",
        "测试数据持久化",
    ).await?;

    let agent_id = agent.id.clone();

    // Execute multiple times
    for i in 0..5 {
        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let _ = ctx.executor.execute_agent(agent.clone()).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        println!("执行 #{}: 历史长度 = {}", i + 1, agent.conversation_history.len());
    }

    // Get history from store
    let history = ctx.store.get_conversation_history(&agent_id, None).await?;
    println!("\n从存储获取的历史: {} 轮次", history.len());

    assert_eq!(history.len(), 5);

    // Verify each turn has required fields
    for (i, turn) in history.iter().enumerate() {
        println!("\n轮次 #{}:", i + 1);
        println!("  执行ID: {}", turn.execution_id);
        println!("  时间戳: {}", turn.timestamp);
        println!("  触发: {}", turn.trigger_type);
        println!("  成功: {}", turn.success);

        assert!(!turn.execution_id.is_empty());
        assert!(turn.timestamp > 0);
        assert!(turn.success);
    }

    println!("\n✅ 持久化测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_context_window_messages() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试上下文窗口消息构建 ===");

    let mut agent = ctx.create_test_agent(
        "上下文测试",
        "监控温度传感器",
    ).await?;

    agent.context_window_size = 3;
    let agent_id = agent.id.clone();

    ctx.store.save_agent(&agent).await?;

    // Execute multiple times
    for i in 0..5 {
        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let _ = ctx.executor.execute_agent(agent.clone()).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        println!("执行 #{}: 历史={}, ContextWindow={}",
            i + 1, agent.conversation_history.len(), agent.context_window_size);
    }

    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();

    // Build conversation messages
    let messages = ctx.executor.build_conversation_messages(
        &agent,
        &[],
        None,
    );

    println!("\n构建的消息数量: {}", messages.len());
    for (i, msg) in messages.iter().enumerate() {
        let content_len = msg.content.as_text().len();
        println!("  #{}: role={:?}, content长度={}", i + 1, msg.role, content_len);
    }

    assert!(messages.len() > 0);

    println!("\n✅ 上下文窗口测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_conversation_turn_structure() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试对话轮次结构 ===");

    let agent = ctx.create_test_agent(
        "结构测试",
        "分析数据趋势",
    ).await?;

    // Execute once
    let record = ctx.executor.execute_agent(agent.clone()).await?;

    println!("执行状态: {:?}", record.status);
    println!("决策过程:");
    println!("  情况分析: {}", record.decision_process.situation_analysis);
    println!("  推理步骤: {}", record.decision_process.reasoning_steps.len());
    println!("  结论: {}", record.decision_process.conclusion);

    // Load conversation history
    let history = ctx.store.get_conversation_history(&agent.id, None).await?;

    assert_eq!(history.len(), 1);

    let turn = &history[0];

    // Verify TurnInput
    println!("\nTurnInput:");
    println!("  数据收集: {} 项", turn.input.data_collected.len());

    // Verify TurnOutput
    println!("\nTurnOutput:");
    println!("  情况分析: {} 字符", turn.output.situation_analysis.len());
    println!("  推理步骤: {} 步", turn.output.reasoning_steps.len());
    println!("  决策: {} 个", turn.output.decisions.len());
    println!("  结论: {} 字符", turn.output.conclusion.len());

    // Verify metadata
    println!("\n元数据:");
    println!("  执行ID: {}", turn.execution_id);
    println!("  时间戳: {}", turn.timestamp);
    println!("  触发: {}", turn.trigger_type);
    println!("  时长: {}ms", turn.duration_ms);
    println!("  成功: {}", turn.success);

    assert!(!turn.execution_id.is_empty());
    assert!(turn.timestamp > 0);
    assert!(turn.duration_ms >= 0);

    println!("\n✅ 对话轮次结构测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_multiple_executions_accumulation() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试多次执行累积 ===");

    let agent = ctx.create_test_agent(
        "累积测试",
        "监控数据变化",
    ).await?;

    let agent_id = agent.id.clone();
    let executions = 10;

    println!("将执行 {} 次...", executions);

    let start = std::time::Instant::now();

    for i in 0..executions {
        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let _ = ctx.executor.execute_agent(agent.clone()).await?;

        if i % 3 == 0 {
            let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
            println!("  执行 #{}: 历史长度 = {}", i + 1, agent.conversation_history.len());
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    let elapsed = start.elapsed();
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();

    println!("\n总执行次数: {}", agent.conversation_history.len());
    println!("总耗时: {:?}", elapsed);
    println!("平均每次: {:?}", elapsed / executions as u32);

    assert_eq!(agent.conversation_history.len(), executions);

    println!("\n✅ 多次执行累积测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_conversation_history_ordering() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试对话历史顺序 ===");

    let agent = ctx.create_test_agent(
        "顺序测试",
        "验证历史顺序",
    ).await?;

    let agent_id = agent.id.clone();
    let mut timestamps = Vec::new();

    // Execute 5 times with delays
    for i in 0..5 {
        let _before = chrono::Utc::now().timestamp();

        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let _ = ctx.executor.execute_agent(agent.clone()).await?;

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        if let Some(turn) = agent.conversation_history.last() {
            timestamps.push(turn.timestamp);
            println!("执行 #{}: 时间戳={}", i + 1, turn.timestamp);
        }
    }

    // Verify timestamps are increasing
    for i in 1..timestamps.len() {
        assert!(timestamps[i] >= timestamps[i - 1],
            "Timestamps should be non-decreasing");
    }

    println!("\n✅ 顺序验证通过！");
    Ok(())
}

#[tokio::test]
async fn test_agent_role_prompts() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试不同类型的Agent提示 ===");

    let agent_configs = vec![
        ("Monitor", "监控传感器数据"),
        ("Executor", "执行控制指令"),
        ("Analyst", "分析数据趋势"),
    ];

    for (name, prompt) in &agent_configs {
        println!("\n--- 类型: {} ---", name);

        let agent = ctx.create_test_agent(
            &format!("{}_agent", name),
            prompt,
        ).await?;

        // Build messages to see the agent-specific prompt
        let messages = ctx.executor.build_conversation_messages(
            &agent,
            &[],
            None,
        );

        // First message should be system prompt
        if let Some(first_msg) = messages.first() {
            println!("系统提示存在: 是");
            println!("角色: {:?}", first_msg.role);

            // Get content for verification
            let content_text = first_msg.content.as_text();
            println!("内容长度: {} 字符", content_text.len());
        }
    }

    println!("\n✅ Agent提示测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_full_lifecycle() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n{}", "============================================================");
    println!("=== 完整 Agent 对话生命周期测试 ===");
    println!("============================================================");

    // Create agents of each type
    let test_agents = vec![
        ("温度监控", "监控温度，超过30度告警"),
        ("开关控制", "高温时打开开关"),
        ("趋势分析", "分析数据趋势"),
    ];

    let mut agent_ids = Vec::new();

    for (name, prompt) in &test_agents {
        let agent = ctx.create_test_agent(name, prompt).await?;
        println!("\n创建: {}", agent.name);
        agent_ids.push((agent.id.clone(), agent.name.clone()));
    }

    println!("\n--- 执行阶段 ---");

    // Execute each agent multiple times
    for (agent_id, name) in &agent_ids {
        println!("\n执行: {}", name);

        for i in 0..3 {
            let agent = ctx.store.get_agent(agent_id).await?.unwrap();
            let record = ctx.executor.execute_agent(agent.clone()).await?;

            println!("  #{}: {:?}", i + 1, record.status);

            let agent = ctx.store.get_agent(agent_id).await?.unwrap();
            assert_eq!(agent.conversation_history.len(), i + 1);
        }

        let agent = ctx.store.get_agent(agent_id).await?.unwrap();
        println!("  ✅ {} 完成 (历史: {} 轮次)", name, agent.conversation_history.len());
    }

    println!("\n--- 验证阶段 ---");

    // Verify all agents have correct history
    for (agent_id, name) in &agent_ids {
        let agent = ctx.store.get_agent(agent_id).await?.unwrap();
        let history = ctx.store.get_conversation_history(agent_id, None).await?;

        println!("\n{}:", name);
        println!("  历史轮次: {}", history.len());
        println!("  上下文窗口: {}", agent.context_window_size);

        assert_eq!(history.len(), 3);
    }

    println!("\n{}", "============================================================");
    println!("✅ 完整生命周期测试全部通过！");
    println!("============================================================");

    Ok(())
}

#[tokio::test]
async fn test_conversation_turn_fields() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试对话轮次字段完整性 ===");

    let agent = ctx.create_test_agent(
        "字段完整性测试",
        "测试所有字段",
    ).await?;

    let _ = ctx.executor.execute_agent(agent.clone()).await?;

    let history = ctx.store.get_conversation_history(&agent.id, None).await?;

    assert_eq!(history.len(), 1);

    let turn = &history[0];

    // Verify all required fields
    assert!(!turn.execution_id.is_empty(), "execution_id should not be empty");
    assert!(turn.timestamp > 0, "timestamp should be positive");
    assert!(!turn.trigger_type.is_empty(), "trigger_type should not be empty");
    assert!(turn.duration_ms >= 0, "duration_ms should be non-negative");

    println!("\n字段验证:");
    println!("  ✓ execution_id: {}", turn.execution_id);
    println!("  ✓ timestamp: {}", turn.timestamp);
    println!("  ✓ trigger_type: {}", turn.trigger_type);
    println!("  ✓ duration_ms: {}", turn.duration_ms);
    println!("  ✓ success: {}", turn.success);

    // Verify TurnOutput fields
    println!("\nTurnOutput 字段:");
    println!("  ✓ situation_analysis: {} 字符", turn.output.situation_analysis.len());
    println!("  ✓ reasoning_steps: {} 项", turn.output.reasoning_steps.len());
    println!("  ✓ decisions: {} 个", turn.output.decisions.len());
    println!("  ✓ conclusion: {} 字符", turn.output.conclusion.len());

    println!("\n✅ 字段完整性测试通过！");
    Ok(())
}
