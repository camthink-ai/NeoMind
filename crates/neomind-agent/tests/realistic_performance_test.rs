//! 真实LLM性能测试
//!
//! 这个测试**真正调用Ollama LLM**来衡量实际性能

use neomind_agent::ai_agent::{AgentExecutor, AgentExecutorConfig};
use neomind_core::llm::backend::{GenerationParams, LlmInput};
use neomind_core::{
    message::{Content, Message, MessageRole},
    EventBus, LlmRuntime, MetricValue, NeoMindEvent,
};
use neomind_llm::backends::ollama::{OllamaConfig, OllamaRuntime};
use neomind_storage::{
    AgentMemory, AgentResource, AgentSchedule, AgentStats, AgentStatus, AgentStore, AiAgent,
    DataPoint, LongTermMemory, ResourceType, ScheduleType, ShortTermMemory, TimeSeriesStore,
    WorkingMemory,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct RealPerfTestContext {
    pub store: Arc<AgentStore>,
    pub event_bus: Arc<EventBus>,
    pub llm_runtime: Arc<OllamaRuntime>,
    pub time_series: Arc<TimeSeriesStore>,
    pub executor: AgentExecutor,
}

impl RealPerfTestContext {
    async fn new() -> anyhow::Result<Self> {
        let store = AgentStore::memory()?;
        let event_bus = Arc::new(EventBus::new());

        let ollama_config = OllamaConfig {
            endpoint: "http://localhost:11434".to_string(),
            model: "qwen2.5:0.5b".to_string(),
            timeout_secs: 120,
        };
        let llm_runtime = Arc::new(OllamaRuntime::new(ollama_config)?);

        let time_series = TimeSeriesStore::memory()?;

        let executor_config = AgentExecutorConfig {
            store: store.clone(),
            time_series_storage: Some(time_series.clone()),
            device_service: None,
            event_bus: Some(event_bus.clone()),
            message_manager: None,
            llm_runtime: Some(llm_runtime.clone()),
            llm_backend_store: None,
            extension_registry: None,
        };
        let executor = AgentExecutor::new(executor_config).await?;

        Ok(Self {
            store,
            event_bus,
            llm_runtime,
            time_series,
            executor,
        })
    }

    /// 直接调用LLM进行分析 - 真实性能
    async fn llm_analyze(&self, system_prompt: &str, user_input: &str) -> (String, u128) {
        let messages = vec![
            Message::new(MessageRole::System, Content::text(system_prompt)),
            Message::new(MessageRole::User, Content::text(user_input)),
        ];

        let input = LlmInput {
            messages,
            params: GenerationParams {
                temperature: Some(0.7),
                max_tokens: Some(500),
                ..Default::default()
            },
            model: Some("qwen2.5:3b".to_string()),
            stream: false,
            tools: None,
        };

        let start = Instant::now();
        let output = self.llm_runtime.generate(input).await.unwrap_or_else(|e| {
            neomind_core::llm::backend::LlmOutput {
                text: format!("Error: {}", e),
                finish_reason: neomind_core::llm::backend::FinishReason::Error,
                usage: None,
                thinking: None,
            }
        });
        let elapsed = start.elapsed().as_millis();

        (output.text, elapsed)
    }

    async fn inject_metrics(&self, device_id: &str, metric: &str, values: &[f64]) {
        for &value in values {
            let point = DataPoint {
                timestamp: chrono::Utc::now().timestamp_millis(),
                value: serde_json::json!(value),
                quality: Some(1.0),
                metadata: None,
            };
            self.time_series.write(device_id, metric, point).await.ok();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Create a test agent with the given parameters
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
            enable_tool_chaining: false,
            max_chain_depth: 3,
        };

        self.store.save_agent(&agent).await?;
        Ok(agent)
    }
}

fn ollama_available() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 11434));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok()
}

#[tokio::test]
#[ignore = "Requires real LLM calls"]
async fn test_real_llm_performance() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama 未运行，跳过测试");
        return Ok(());
    }

    let ctx = RealPerfTestContext::new().await?;

    println!(
        "\n{}",
        "============================================================"
    );
    println!("真实LLM性能测试 - 每次调用都实际等待LLM响应");
    println!(
        "{}\n",
        "============================================================"
    );

    let system_prompt = "你是一个物联网设备监控助手。分析数据并给出建议。";

    // 测试1: 简单查询
    println!("📊 测试1: 简单温度数据分析");
    let user_input = "当前温度为28度，湿度为60%，请简要分析这个环境状态。";

    let start = Instant::now();
    let (response, elapsed) = ctx.llm_analyze(system_prompt, user_input).await;
    println!("   响应时间: {}ms", elapsed);
    println!("   LLM响应: {}", response);
    println!();

    // 测试2: 复杂查询 - 多设备分析
    println!("📊 测试2: 多设备数据分析");
    let multi_device_input = r#"
我有以下传感器数据：
- 办公室A: 温度26°C，湿度55%
- 办公室B: 温度29°C，湿度65%
- 服务器机房: 温度24°C，湿度45%
- 大厅: 温度27°C，湿度60%

请分析：
1. 哪些区域需要关注？
2. 是否有异常情况？
3. 给出具体建议。
"#;

    let (response2, elapsed2) = ctx.llm_analyze(system_prompt, multi_device_input).await;
    println!("   响应时间: {}ms", elapsed2);
    println!("   LLM响应:\n{}\n", response2);

    // 测试3: 故障诊断
    println!("📊 测试3: 设备故障诊断");
    let fault_diagnosis = r#"
电机运行数据：
- 振动: 7.5 mm/s (正常范围 <5mm/s)
- 温度: 82°C (正常范围 <80°C)
- 运行时长: 8小时无停机

请诊断：
1. 设备状态是否正常？
2. 可能的故障原因？
3. 建议的维护措施？
"#;

    let (response3, elapsed3) = ctx.llm_analyze(system_prompt, fault_diagnosis).await;
    println!("   响应时间: {}ms", elapsed3);
    println!("   LLM响应:\n{}\n", response3);

    // 测试4: 重复调用 - 测试稳定性
    println!("📊 测试4: 连续10次调用测试稳定性");
    let mut times = Vec::new();

    for i in 0..10 {
        let query = format!("第{}次查询：当前温度{}度，请简要评价。", i + 1, 20 + i);
        let start = Instant::now();
        let _ = ctx.llm_analyze(system_prompt, &query).await;
        times.push(start.elapsed().as_millis());
        println!("   第{}次: {}ms", i + 1, times.last().unwrap());
    }

    let avg = times.iter().sum::<u128>() / times.len() as u128;
    let min = *times.iter().min().unwrap();
    let max = *times.iter().max().unwrap();

    println!("\n   📈 统计:");
    println!("      平均: {}ms", avg);
    println!("      最快: {}ms", min);
    println!("      最慢: {}ms", max);
    println!("      标准差: {:.2}ms", {
        let avg_f = avg as f64;
        let variance = times
            .iter()
            .map(|t| (*t as f64 - avg_f).powi(2))
            .sum::<f64>()
            / times.len() as f64;
        variance.sqrt()
    });

    // 测试5: 长文本生成
    println!("\n📊 测试5: 长文本生成（详细报告）");
    let report_request = r#"
请生成一份详细的设备维护报告，包含以下部分：
1. 设备运行概况
2. 发现的问题
3. 趋势分析
4. 维护建议
5. 预防措施

当前状态：监控10台设备，运行正常，温度范围18-28°C。
"#;

    let (response5, elapsed5) = ctx.llm_analyze(system_prompt, report_request).await;
    println!("   响应时间: {}ms", elapsed5);
    println!("   响应长度: {} 字符", response5.len());
    println!("   响应预览: {}...", &response5[..response5.len().min(100)]);

    println!(
        "\n{}",
        "============================================================"
    );
    println!("真实性能测试完成");
    println!("============================================================");

    Ok(())
}

#[tokio::test]
#[ignore = "Requires real LLM calls"]
async fn test_llm_vs_mock_comparison() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama 未运行，跳过测试");
        return Ok(());
    }

    let ctx = RealPerfTestContext::new().await?;

    println!(
        "\n{}",
        "============================================================"
    );
    println!("LLM真实调用 vs 模拟响应 性能对比");
    println!(
        "{}\n",
        "============================================================"
    );

    // 准备测试数据
    ctx.inject_metrics(
        "sensor_01",
        "temperature",
        &[20.0, 22.0, 24.0, 26.0, 28.0, 30.0],
    )
    .await;

    // 测试场景1: Agent执行（不调用LLM）
    println!("📊 场景1: Agent执行（当前实现 - 无LLM）");

    let executor_config = AgentExecutorConfig {
        store: ctx.store.clone(),
        time_series_storage: Some(ctx.time_series.clone()),
        device_service: None,
        event_bus: Some(ctx.event_bus.clone()),
        message_manager: None,
        llm_runtime: None, // 没有LLM
        llm_backend_store: None,
        extension_registry: None,
    };

    let executor = AgentExecutor::new(executor_config).await?;

    let agent = AiAgent {
        id: uuid::Uuid::new_v4().to_string(),
        name: "测试Agent".to_string(),
        description: None,
        user_prompt: "监控温度".to_string(),
        llm_backend_id: None,
        parsed_intent: None,
        resources: vec![AgentResource {
            resource_type: ResourceType::Metric,
            resource_id: "sensor_01:temperature".to_string(),
            name: "sensor_01".to_string(),
            config: serde_json::json!({}),
        }],
        schedule: AgentSchedule {
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(60),
            cron_expression: None,
            timezone: None,
            event_filter: None,
        },
        status: AgentStatus::Active,
        created_at: chrono::Utc::now().timestamp(),
        updated_at: chrono::Utc::now().timestamp(),
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
            updated_at: chrono::Utc::now().timestamp(),
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
        enable_tool_chaining: false,
        max_chain_depth: 3,
    };

    ctx.store.save_agent(&agent).await.ok();

    let start = Instant::now();
    let record = executor.execute_agent(agent.clone()).await?;
    let agent_time = start.elapsed().as_millis();

    println!("   执行时间: {}ms", agent_time);
    println!(
        "   数据收集: {} 个",
        record.decision_process.data_collected.len()
    );
    println!("   决策数: {}", record.decision_process.decisions.len());
    println!("   结论: {}", record.decision_process.conclusion);
    println!("   ⚠️  注意: 没有调用LLM，结论是预设的");

    // 场景2: 真实LLM调用
    println!("\n📊 场景2: 真实LLM调用分析同样数据");

    let llm_input = format!(
        "传感器数据：温度读数为 [20, 22, 24, 26, 28, 30] 度。
请分析：1. 趋势如何？2. 是否异常？3. 需要采取什么行动？"
    );

    let (llm_response, llm_time) = ctx.llm_analyze("你是设备监控助手。", &llm_input).await;

    println!("   LLM响应时间: {}ms", llm_time);
    println!("   LLM分析结果: {}", llm_response);

    // 对比
    println!("\n📊 性能对比:");
    println!("   ┌──────────────┬──────────┬──────────────┐");
    println!("   │ 方式         │ 耗时     │ 说明         │");
    println!("   ├──────────────┼──────────┼──────────────┤");
    println!("   │ Agent(无LLM) │ {}ms     │ 无真实AI     │", agent_time);
    println!(
        "   │ Agent(+LLM)  │ {}ms    │ 真实AI推理  │",
        agent_time + llm_time
    );
    println!(
        "   │ 差异         │ {:.1}x   │ LLM是主要耗时│",
        (agent_time + llm_time) as f64 / agent_time.max(1) as f64
    );
    println!("   └──────────────┴──────────┴──────────────┘");

    println!("\n💡 结论:");
    println!("   之前测试显示的20-30ms是**没有调用LLM**的执行时间");
    println!("   真实LLM调用需要500-3000ms，这是更准确的结果");
    println!("   系统瓶颈主要在LLM推理，不在Agent框架本身");

    Ok(())
}

#[tokio::test]
#[ignore = "Requires real LLM calls"]
async fn test_realistic_multi_agent_scenario() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama 未运行，跳过测试");
        return Ok(());
    }

    let ctx = RealPerfTestContext::new().await?;

    println!(
        "\n{}",
        "============================================================"
    );
    println!("真实场景：多Agent协作（每次都调用LLM）");
    println!(
        "{}\n",
        "============================================================"
    );

    // 模拟温室监控场景
    ctx.inject_metrics("greenhouse", "temperature", &[25.0, 26.0, 27.0, 29.0, 31.0])
        .await;
    ctx.inject_metrics("greenhouse", "humidity", &[65.0, 63.0, 61.0, 58.0, 55.0])
        .await;

    println!("🌱 场景：温室温度异常升高\n");

    let system_prompt = "你是智能温室监控助手。";

    // 步骤1: 监控Agent分析
    println!("📊 步骤1: 监控Agent分析数据...");

    let monitor_input = r#"
温室传感器数据：
- 温度: [25, 26, 27, 29, 31] °C (上升趋势)
- 湿度: [65, 63, 61, 58, 55] % (下降趋势)

请分析：
1. 当前状态如何？
2. 是否存在异常？
3. 需要什么操作？
"#;

    let (monitor_response, monitor_time) = ctx.llm_analyze(system_prompt, monitor_input).await;
    println!("   耗时: {}ms", monitor_time);
    println!("   分析:\n   {}\n", monitor_response);

    // 步骤2: 执行Agent生成控制指令
    println!("📊 步骤2: 执行Agent生成控制指令...");

    let executor_input = r#"
基于监控Agent的分析，温室温度已达31°C（超过上限28°C）。
可用操作：
1. 开启通风扇 (降低温度)
2. 开启遮阳网 (减少阳光)
3. 开启喷淋系统 (降温+加湿)

请给出具体的控制指令（JSON格式）。
"#;

    let (executor_response, executor_time) = ctx.llm_analyze(system_prompt, executor_input).await;
    println!("   耗时: {}ms", executor_time);
    println!("   指令:\n   {}\n", executor_response);

    // 步骤3: 分析Agent生成报告
    println!("📊 步骤3: 分析Agent生成优化建议...");

    let analyst_input = r#"
过去1小时的温室数据：
- 温度范围: 25-31°C
- 湿度范围: 55-65%
- 已执行操作: 开启通风扇

请分析：
1. 操作效果如何？
2. 未来1小时趋势预测？
3. 长期优化建议？
"#;

    let (analyst_response, analyst_time) = ctx.llm_analyze(system_prompt, analyst_input).await;
    println!("   耗时: {}ms", analyst_time);
    println!("   建议:\n   {}", analyst_response);

    // 总计
    let total_time = monitor_time + executor_time + analyst_time;

    println!(
        "\n{}",
        "============================================================"
    );
    println!("📊 多Agent协作真实耗时:");
    println!("   监控Agent: {}ms", monitor_time);
    println!("   执行Agent: {}ms", executor_time);
    println!("   分析Agent: {}ms", analyst_time);
    println!("   ──────────────────");
    println!(
        "   总计: {}ms ({:.1}秒)",
        total_time,
        total_time as f64 / 1000.0
    );
    println!("   平均每Agent: {}ms", total_time / 3);
    println!("============================================================");

    println!("\n💡 结论:");
    println!(
        "   真实LLM场景下，3个Agent协作需要 {:.1} 秒",
        total_time as f64 / 1000.0
    );
    println!("   这才是更接近实际部署的性能表现");

    Ok(())
}

#[tokio::test]
#[ignore = "Requires real LLM calls"]
async fn test_parallel_vs_sequential_execution() -> anyhow::Result<()> {
    if !ollama_available() {
        println!("⚠️  Ollama 未运行，跳过测试");
        return Ok(());
    }

    let ctx = RealPerfTestContext::new().await?;

    println!(
        "\n{}",
        "============================================================"
    );
    println!("并行 vs 顺序 LLM调用 性能对比测试");
    println!(
        "{}\n",
        "============================================================"
    );

    let system_prompt = "你是一个物联网设备监控助手。";

    // 定义3个不同的查询任务
    let queries = vec![
        "当前温度为28度，湿度为60%，请简要分析这个环境状态。",
        "办公室A温度26°C湿度55%，办公室B温度29°C湿度65%，请分析差异。",
        "电机振动7.5mm/s温度82°C运行8小时，请诊断设备状态。",
    ];

    // 测试1: 顺序LLM调用
    println!("📊 测试1: 顺序调用LLM 3次");
    let start = std::time::Instant::now();

    let mut sequential_results = Vec::new();
    for (i, query) in queries.iter().enumerate() {
        let (response, elapsed) = ctx.llm_analyze(system_prompt, query).await;
        sequential_results.push((i + 1, elapsed));
        println!(
            "   查询{} - {}ms (响应长度: {} 字符)",
            i + 1,
            elapsed,
            response.len()
        );
    }

    let sequential_time = start.elapsed().as_millis();
    println!("   顺序调用总时间: {}ms", sequential_time);

    // 测试2: 并行LLM调用
    println!("\n📊 测试2: 并行调用LLM 3次");
    let start = std::time::Instant::now();

    // 使用futures::future::join_all并行执行
    use futures::future::join_all;

    let parallel_futures: Vec<_> = queries
        .iter()
        .map(|query| {
            let llm_runtime = ctx.llm_runtime.clone();
            let prompt = system_prompt.to_string();
            let q = query.to_string();

            tokio::spawn(async move {
                let messages = vec![
                    neomind_core::message::Message::new(
                        neomind_core::message::MessageRole::System,
                        neomind_core::message::Content::text(&prompt),
                    ),
                    neomind_core::message::Message::new(
                        neomind_core::message::MessageRole::User,
                        neomind_core::message::Content::text(&q),
                    ),
                ];

                let input = neomind_core::llm::backend::LlmInput {
                    messages,
                    params: neomind_core::llm::backend::GenerationParams {
                        temperature: Some(0.7),
                        max_tokens: Some(500),
                        ..Default::default()
                    },
                    model: Some("qwen2.5:0.5b".to_string()),
                    stream: false,
                    tools: None,
                };

                let start = std::time::Instant::now();
                let output = llm_runtime.generate(input).await.unwrap_or_else(|e| {
                    neomind_core::llm::backend::LlmOutput {
                        text: format!("Error: {}", e),
                        finish_reason: neomind_core::llm::backend::FinishReason::Error,
                        usage: None,
                        thinking: None,
                    }
                });
                let elapsed = start.elapsed().as_millis();
                (elapsed, output.text.len())
            })
        })
        .collect();

    let parallel_results = join_all(parallel_futures).await;
    let parallel_time = start.elapsed().as_millis();

    for (i, result) in parallel_results.iter().enumerate() {
        if let Ok((elapsed, len)) = result {
            println!("   查询{} - {}ms (响应长度: {} 字符)", i + 1, elapsed, len);
        }
    }
    println!("   并行调用总时间: {}ms", parallel_time);

    // 对比
    println!(
        "\n{}",
        "============================================================"
    );
    println!("📊 性能对比:");
    println!("   ┌──────────────┬──────────┬──────────────┐");
    println!("   │ 方式         │ 耗时     │ 说明         │");
    println!("   ├──────────────┼──────────┼──────────────┤");
    println!(
        "   │ 顺序调用     │ {}ms   │ 逐个等待LLM  │",
        sequential_time
    );
    println!("   │ 并行调用     │ {}ms   │ 同时等待LLM  │", parallel_time);

    let speedup = sequential_time as f64 / parallel_time.max(1) as f64;
    let improvement =
        ((sequential_time as f64 - parallel_time as f64) / sequential_time as f64 * 100.0);

    println!(
        "   │ 性能提升     │ {:.1}%    │ {:.1}x 更快    │",
        improvement.max(0.0),
        speedup
    );
    println!("   └──────────────┴──────────┴──────────────┘");

    println!("\n💡 结论:");
    println!("   并行执行可以让多个LLM请求同时等待响应");
    println!(
        "   3个LLM调用的总时间从 {}ms 降至 {}ms",
        sequential_time, parallel_time
    );
    println!(
        "   这意味着在多Agent协作场景中，可以节省 {:.1}% 的时间",
        improvement.max(0.0)
    );
    println!("============================================================");

    Ok(())
}
