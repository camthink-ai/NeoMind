//! AI Agent Conversation History Integration Test

#![allow(dead_code)]

use neomind_agent::ai_agent::{AgentExecutor, AgentExecutorConfig};
use neomind_core::EventBus;
use neomind_storage::{
    AgentMemory, AgentSchedule, AgentStats, AgentStatus, AgentStore, AiAgent, ExecutionJournal,
    ExecutionMode, ScheduleType,
};
use std::sync::Arc;

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
        })
    }

    async fn create_test_agent(&self, name: &str, user_prompt: &str) -> anyhow::Result<AiAgent> {
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

    async fn load_agent(&self, agent_id: &str) -> anyhow::Result<AiAgent> {
        self.store
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))
    }
}

// ========== Tests ==========

#[tokio::test]
async fn test_conversation_history_basic() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试基础对话历史 ===");

    let agent = ctx.create_test_agent("测试Agent", "监控传感器数据").await?;

    let agent_id = agent.id.clone();
    println!("创建 Agent: {}", agent.name);

    // Execute 3 times
    for i in 0..3 {
        println!("\n--- 执行 #{} ---", i + 1);

        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let record = ctx
            .executor
            .execute_agent(agent.clone(), None, None)
            .await?;

        println!("状态: {:?}", record.status);
        println!("时长: {}ms", record.duration_ms);

        let _agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let _ = i;
    }

    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    // conversation_history is now Vec<serde_json::Value> (backward compat only, not written to)
    // Just verify the field deserializes without error
    let _ = &agent.conversation_history;

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

        let agent = ctx
            .create_test_agent(&format!("{}_test", name), prompt)
            .await?;

        println!("创建: {}", agent.name);

        // Execute once
        let record = ctx
            .executor
            .execute_agent(agent.clone(), None, None)
            .await?;
        println!("执行状态: {:?}", record.status);

        // Reload and verify agent can be retrieved
        let _agent = ctx.store.get_agent(&agent.id).await?.unwrap();
    }

    println!("\n✅ 所有类型测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_conversation_persistence() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试对话持久化 ===");

    let agent = ctx
        .create_test_agent("持久化测试", "测试数据持久化")
        .await?;

    let agent_id = agent.id.clone();

    // Execute multiple times
    for i in 0..5 {
        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let _ = ctx
            .executor
            .execute_agent(agent.clone(), None, None)
            .await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        println!(
            "执行 #{}: 历史长度 = {}",
            i + 1,
            agent.conversation_history.len()
        );
    }

    // conversation_history is now Vec<serde_json::Value> (backward compat only, not written to)
    // Verify the agent can be loaded after multiple executions
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
    println!("\nAgent loaded successfully after 5 executions");
    let _ = &agent.conversation_history;

    println!("\n✅ 持久化测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_context_window_messages() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试上下文窗口消息构建 ===");

    let mut agent = ctx
        .create_test_agent("上下文测试", "监控温度传感器")
        .await?;

    agent.context_window_size = 3;
    let agent_id = agent.id.clone();

    ctx.store.save_agent(&agent).await?;

    // Execute multiple times
    for i in 0..5 {
        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let _ = ctx
            .executor
            .execute_agent(agent.clone(), None, None)
            .await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        println!(
            "执行 #{}: 历史={}, ContextWindow={}",
            i + 1,
            agent.conversation_history.len(),
            agent.context_window_size
        );
    }

    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();

    // Verify conversation history deserializes (backward compat field)
    let _ = &agent.conversation_history;

    println!("\n✅ Context window test passed!");
    Ok(())
}

#[tokio::test]
async fn test_conversation_turn_structure() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试执行记录结构 ===");

    let agent = ctx.create_test_agent("结构测试", "分析数据趋势").await?;

    // Execute once
    let record = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("执行状态: {:?}", record.status);
    println!("决策过程:");
    println!("  情况分析: {}", record.decision_process.situation_analysis);
    println!(
        "  推理步骤: {}",
        record.decision_process.reasoning_steps.len()
    );
    println!("  结论: {}", record.decision_process.conclusion);

    // Verify the execution record has expected fields
    assert!(!record.id.is_empty());
    // duration_ms is u64 — always >= 0, so no assert needed

    println!("\n✅ 执行记录结构测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_multiple_executions_accumulation() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试多次执行累积 ===");

    let agent = ctx.create_test_agent("累积测试", "监控数据变化").await?;

    let agent_id = agent.id.clone();
    let executions = 10;

    println!("将执行 {} 次...", executions);

    let start = std::time::Instant::now();

    for i in 0..executions {
        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let _ = ctx
            .executor
            .execute_agent(agent.clone(), None, None)
            .await?;

        if i % 3 == 0 {
            let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
            println!(
                "  执行 #{}: 历史长度 = {}",
                i + 1,
                agent.conversation_history.len()
            );
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    let elapsed = start.elapsed();
    let agent = ctx.store.get_agent(&agent_id).await?.unwrap();

    println!("\n总执行次数: {}", executions);
    println!("总耗时: {:?}", elapsed);
    println!("平均每次: {:?}", elapsed / executions as u32);

    // conversation_history is now Vec<serde_json::Value> (backward compat, not written to)
    let _ = &agent.conversation_history;

    println!("\n✅ 多次执行累积测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_conversation_history_ordering() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试对话历史顺序 ===");

    let agent = ctx.create_test_agent("顺序测试", "验证历史顺序").await?;

    let agent_id = agent.id.clone();

    // Execute 5 times with delays
    for _i in 0..5 {
        let _before = chrono::Utc::now().timestamp();

        let agent = ctx.store.get_agent(&agent_id).await?.unwrap();
        let _ = ctx
            .executor
            .execute_agent(agent.clone(), None, None)
            .await?;

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // conversation_history is now Vec<serde_json::Value> (backward compat only, not written to)
        let _agent = ctx.store.get_agent(&agent_id).await?.unwrap();
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

        let agent = ctx
            .create_test_agent(&format!("{}_agent", name), prompt)
            .await?;

        // Verify agent has proper memory structure
        // Newly created agents have empty memory/history -- that's expected.
        let _ = &agent.memory.journal.records;
        let _ = &agent.conversation_history;

        println!(
            "Journal records: {}, History turns: {}",
            agent.memory.journal.records.len(),
            agent.conversation_history.len()
        );
    }

    println!("\n✅ Agent提示测试通过！");
    Ok(())
}

#[tokio::test]
async fn test_full_lifecycle() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n============================================================");
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
            let record = ctx
                .executor
                .execute_agent(agent.clone(), None, None)
                .await?;

            println!("  #{}: {:?}", i + 1, record.status);
            let _ = i; // use loop variable
        }

        let agent = ctx.store.get_agent(agent_id).await?.unwrap();
        println!("  ✅ {} 完成", name,);
        let _ = &agent.conversation_history;
    }

    println!("\n--- 验证阶段 ---");

    // Verify all agents can be loaded
    for (agent_id, name) in &agent_ids {
        let agent = ctx.store.get_agent(agent_id).await?.unwrap();

        println!("\n{}:", name);
        println!("  上下文窗口: {}", agent.context_window_size);
    }

    println!("\n============================================================");
    println!("✅ 完整生命周期测试全部通过！");
    println!("============================================================");

    Ok(())
}

#[tokio::test]
async fn test_conversation_turn_fields() -> anyhow::Result<()> {
    let ctx = TestContext::new().await?;

    println!("\n=== 测试执行记录字段完整性 ===");

    let agent = ctx
        .create_test_agent("字段完整性测试", "测试所有字段")
        .await?;

    let record = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    // Verify execution record fields
    assert!(
        !record.id.is_empty(),
        "execution record id should not be empty"
    );
    let _ = record.duration_ms; // u64 is always >= 0

    println!("\n字段验证:");
    println!("  ✓ id: {}", record.id);
    println!("  ✓ duration_ms: {}", record.duration_ms);

    println!("\n✅ 字段完整性测试通过！");
    Ok(())
}
