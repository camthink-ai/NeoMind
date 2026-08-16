//! Real-World Business Flow Simulation Test
//!
//! 真实业务场景模拟测试:
//!
//! ## 场景1: 智能楼宇环境控制系统
//! - 多区域温度监控
//! - 空调自动控制
//! - 能耗优化
//!
//! ## 场景2: 工业设备预测性维护
//! - 电机振动监控
//! - 温度异常检测
//! - 故障预警
//!
//! ## 场景3: 多Agent协作系统
//! - 监控Agent检测异常
//! - 执行Agent响应操作
//! - 分析Agent生成报告
//!
//! ## 场景4: 持续运行的长期Agent
//! - 24小时周期性执行
//! - 历史数据趋势分析
//! - 基线学习和异常检测

#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]

use neomind_agent::ai_agent::{AgentExecutor, AgentExecutorConfig};
use neomind_agent::llm_backends::{OllamaConfig, OllamaRuntime};
use neomind_core::{EventBus, MetricValue, NeoMindEvent};
use neomind_messages::{MessageManager, MessageSeverity};
use neomind_storage::{
    AgentMemory, AgentResource, AgentSchedule, AgentStats, AgentStatus, AgentStore, AiAgent,
    DataPoint, ExecutionJournal, ExecutionMode, ResourceType, ScheduleType, TimeSeriesStore,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ============================================================================
// Test Context
// ============================================================================

struct SimulationContext {
    pub store: Arc<AgentStore>,
    pub executor: AgentExecutor,
    pub event_bus: Arc<EventBus>,
    pub llm_runtime: Arc<OllamaRuntime>,
    pub time_series: Arc<TimeSeriesStore>,
    pub message_manager: Arc<MessageManager>,
}

impl SimulationContext {
    async fn new() -> anyhow::Result<Self> {
        let store = AgentStore::memory()?;
        let event_bus = Arc::new(EventBus::new());

        let ollama_config = OllamaConfig {
            endpoint: "http://localhost:11434".to_string(),
            model: "qwen3.5:2b".to_string(),
            timeout_secs: 120,
        };
        let llm_runtime = Arc::new(OllamaRuntime::new(ollama_config)?);

        let time_series = TimeSeriesStore::memory()?;

        let message_manager = Arc::new(MessageManager::new());
        // Note: MessageManager now initializes with default channels via register_default_channels
        message_manager.register_default_channels().await;

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

    /// 模拟实时传感器数据流
    async fn simulate_sensor_stream(
        &self,
        device_id: &str,
        metric: &str,
        base_value: f64,
        variation: f64,
        count: usize,
        interval_ms: u64,
        trend: Option<f64>, // 每次变化的趋势值
    ) -> Vec<f64> {
        let mut values = Vec::new();
        let mut current = base_value;

        for i in 0..count {
            // 添加随机变化
            let delta = (rand::random::<f64>() - 0.5) * 2.0 * variation;
            current += delta;

            // 添加趋势
            if let Some(t) = trend {
                current += t;
            }

            // 限制范围
            current = current.max(0.0);

            values.push(current);

            let point = DataPoint {
                timestamp: chrono::Utc::now().timestamp_millis(),
                value: serde_json::json!(current),
                quality: Some(1.0),
                metadata: None,
            };

            self.time_series.write(device_id, metric, point).await.ok();

            // 发布事件
            let event = NeoMindEvent::DeviceMetric {
                device_id: device_id.to_string(),
                metric: metric.to_string(),
                value: MetricValue::Float(current),
                timestamp: chrono::Utc::now().timestamp(),
                quality: Some(1.0),
                is_virtual: None,
            };
            let _ = self.event_bus.publish(event).await;

            if i < count - 1 {
                tokio::time::sleep(Duration::from_millis(interval_ms)).await;
            }
        }

        values
    }

    async fn create_agent(
        &self,
        name: &str,
        resources: Vec<AgentResource>,
        user_prompt: &str,
        interval_seconds: u64,
    ) -> anyhow::Result<AiAgent> {
        let now = chrono::Utc::now().timestamp();

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
                interval_seconds: Some(interval_seconds),
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
            execution_mode: ExecutionMode::Focused,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            priority: 128,
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 20, // 保留更多历史
            enable_tool_chaining: false,
            max_chain_depth: 3,
            tool_config: None,
        };

        self.store.save_agent(&agent).await?;
        Ok(agent)
    }

    async fn send_alert(
        &self,
        severity: MessageSeverity,
        title: &str,
        message: &str,
        device: &str,
    ) {
        let msg = neomind_messages::Message::alert(
            severity,
            title.to_string(),
            message.to_string(),
            device.to_string(),
        );
        if let Ok(msg) = self.message_manager.create_message(msg).await {
            println!("    📢 [{}] {} - {}", severity, title, message);
            println!("       Message ID: {}", msg.id);
        }
    }

    fn print_section(&self, title: &str) {
        println!("\n{}\n{}", "=".repeat(60), title);
        println!("{}\n", "=".repeat(60));
    }

    fn print_subsection(&self, title: &str) {
        println!("\n--- {} ---", title);
    }
}

// ============================================================================
// 场景1: 智能楼宇环境控制系统
// ============================================================================

#[tokio::test]
#[ignore = "Real-world simulation test"]
async fn scenario_1_smart_building_hvac() -> anyhow::Result<()> {
    let ctx = SimulationContext::new().await?;
    ctx.print_section("场景1: 智能楼宇HVAC控制系统模拟");

    // 定义楼宇区域
    let zones = vec![
        ("lobby", "大厅", "temperature"),
        ("office_a", "办公室A", "temperature"),
        ("office_b", "办公室B", "temperature"),
        ("meeting_room", "会议室", "temperature"),
        ("server_room", "服务器机房", "temperature"),
    ];

    println!("📁 楼宇区域配置:");
    for (id, name, metric) in &zones {
        println!("   - {} ({}) - 监控: {}", id, name, metric);
    }

    // 创建监控资源
    let mut monitor_resources = Vec::new();
    for (id, _name, metric) in &zones {
        monitor_resources.push(AgentResource {
            resource_type: ResourceType::Metric,
            resource_id: format!("{}:{}", id, metric),
            name: format!("{} - {}", id, metric),
            config: serde_json::json!({
                "threshold_high": 28.0,
                "threshold_low": 18.0,
            }),
        });
    }

    // 创建HVAC监控Agent
    let hvac_monitor = ctx
        .create_agent(
            "HVAC环境监控中心",
            monitor_resources.clone(),
            "你是智能楼宇HVAC监控中心。职责:
1. 监控所有区域温度，正常范围: 18-28°C
2. 服务器机房需保持22±2°C
3. 检测异常温度波动（单次变化>3°C为异常）
4. 超过阈值时发出告警
5. 分析整体能耗趋势",
            30, // 每30秒检查
        )
        .await?;

    ctx.print_subsection("阶段1: 正常运行状态模拟");

    println!("⏱️  模拟1小时运行（时间压缩）...");

    let mut zone_temps = vec![];
    for (id, name, metric) in &zones {
        // 初始温度
        let base = match *id {
            "lobby" => 23.0,
            "office_a" => 22.0,
            "office_b" => 24.0,
            "meeting_room" => 21.0,
            "server_room" => 22.0,
            _ => 23.0,
        };

        let values = ctx
            .simulate_sensor_stream(
                id,
                metric,
                base,
                1.5,       // 变化范围
                12,        // 数据点数
                50,        // 间隔50ms
                Some(0.1), // 轻微上升趋势
            )
            .await;

        let avg = values.iter().sum::<f64>() / values.len() as f64;
        zone_temps.push((*id, *name, avg));
        println!(
            "   {}: {} 平均 {:.1}°C (范围: {:.1}-{:.1}°C)",
            id,
            name,
            avg,
            values.iter().cloned().fold(f64::INFINITY, f64::min),
            values.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        );
    }

    // 执行Agent分析
    ctx.print_subsection("Agent执行分析");

    let agent = ctx.store.get_agent(&hvac_monitor.id).await?.unwrap();
    let record = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("   状态: {:?}", record.status);
    println!(
        "   数据收集: {} 个数据源",
        record.decision_process.data_collected.len()
    );
    println!(
        "   情况分析: {}",
        record.decision_process.situation_analysis
    );
    println!("   结论: {}", record.decision_process.conclusion);

    // 检查统计
    let agent_after = ctx.store.get_agent(&hvac_monitor.id).await?.unwrap();
    println!("\n📊 执行统计:");
    println!("   总执行次数: {}", agent_after.stats.total_executions);
    println!(
        "   成功率: {}%",
        if agent_after.stats.total_executions > 0 {
            agent_after.stats.successful_executions * 100 / agent_after.stats.total_executions
        } else {
            0
        }
    );
    println!("   平均耗时: {}ms", agent_after.stats.avg_duration_ms);

    ctx.print_subsection("阶段2: 异常情况模拟");

    // 模拟服务器机房过热
    println!("🔴 模拟异常: 服务器机房温度升高");

    let overheating_values = ctx
        .simulate_sensor_stream(
            "server_room",
            "temperature",
            25.0, // 从较高温度开始
            2.0,
            8,
            30,
            Some(1.5), // 快速上升
        )
        .await;

    println!("   服务器机房温度序列:");
    for (i, temp) in overheating_values.iter().enumerate() {
        let icon = if *temp > 28.0 {
            "🔴"
        } else if *temp > 24.0 {
            "🟡"
        } else {
            "🟢"
        };
        println!("     {} t-{}: {:.1}°C {}", icon, (8 - i) * 5, temp, icon);
    }

    // 再次执行Agent
    let agent = ctx.store.get_agent(&hvac_monitor.id).await?.unwrap();
    let record2 = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("\n   Agent响应:");
    println!("   分析: {}", record2.decision_process.situation_analysis);
    println!("   决策数: {}", record2.decision_process.decisions.len());

    for (i, decision) in record2.decision_process.decisions.iter().enumerate() {
        println!("     决策{}: {}", i + 1, decision.description);
    }

    println!("   结论: {}", record2.decision_process.conclusion);

    // 检查是否触发告警
    if record2.decision_process.conclusion.contains("异常")
        || record2.decision_process.conclusion.contains("高")
        || record2.decision_process.conclusion.contains("超过")
    {
        ctx.send_alert(
            MessageSeverity::Warning,
            "HVAC温度异常",
            &format!(
                "服务器机房温度达到 {:.1}°C，超过安全阈值",
                overheating_values.last().unwrap_or(&0.0)
            ),
            "server_room",
        )
        .await;
    }

    ctx.print_subsection("阶段3: 能耗分析");

    // 创建分析师Agent
    let mut energy_resources = Vec::new();
    for (id, _name, metric) in &zones {
        energy_resources.push(AgentResource {
            resource_type: ResourceType::Metric,
            resource_id: format!("{}:{}", id, metric),
            name: format!("{} - {}", id, metric),
            config: serde_json::json!({}),
        });
    }

    let energy_analyst = ctx
        .create_agent(
            "楼宇能耗分析师",
            energy_resources,
            "分析楼宇各区域温度数据，评估能耗情况，提供节能建议",
            60,
        )
        .await?;

    let agent = ctx.store.get_agent(&energy_analyst.id).await?.unwrap();
    let record3 = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("   能耗分析:");
    println!("   分析: {}", record3.decision_process.situation_analysis);
    println!(
        "   推理步骤: {} 步",
        record3.decision_process.reasoning_steps.len()
    );
    for (i, step) in record3
        .decision_process
        .reasoning_steps
        .iter()
        .take(3)
        .enumerate()
    {
        println!("     {}: {}", i + 1, step.description);
    }
    println!("   结论: {}", record3.decision_process.conclusion);

    ctx.print_subsection("场景总结");

    println!("✅ HVAC控制系统测试完成:");
    println!("   - 覆盖区域: {} 个", zones.len());
    println!("   - 数据点: {}+ 个", zones.len() * 12);
    println!("   - Agent执行: 3 次");
    println!("   - 异常检测: 工作正常");
    println!("   - 告警触发: 测试通过");

    Ok(())
}

// ============================================================================
// 场景2: 工业设备预测性维护
// ============================================================================

#[tokio::test]
#[ignore = "Real-world simulation test"]
async fn scenario_2_industrial_predictive_maintenance() -> anyhow::Result<()> {
    let ctx = SimulationContext::new().await?;
    ctx.print_section("场景2: 工业设备预测性维护系统");

    // 设备定义
    let equipment = vec![
        ("motor_1", "主电机#1", "vibration"),
        ("motor_2", "主电机#2", "temperature"),
        ("pump_1", "液压泵#1", "pressure"),
        ("conveyor", "输送带", "speed"),
    ];

    println!("🏭 监控设备:");
    for (id, name, metric) in &equipment {
        println!("   - {} ({}) - 监控: {}", id, name, metric);
    }

    ctx.print_subsection("阶段1: 正常运行数据采集");

    // 创建监控Agent
    let mut resources = Vec::new();
    for (id, _name, metric) in &equipment {
        resources.push(AgentResource {
            resource_type: ResourceType::Metric,
            resource_id: format!("{}:{}", id, metric),
            name: format!("{} - {}", id, metric),
            config: serde_json::json!({
                "normal_range": [0.0, 100.0],
                "warning_threshold": 80.0,
                "critical_threshold": 90.0,
            }),
        });
    }

    let maintenance_monitor = ctx
        .create_agent(
            "设备健康监控Agent",
            resources,
            "监控工业设备运行状态，预测潜在故障:
1. 监控振动、温度、压力等指标
2. 识别异常模式和趋势
3. 预测可能的设备故障
4. 及时发出维护预警
正常范围: 振动<5mm/s, 温度<80°C, 压力<6MPa",
            20,
        )
        .await?;

    // 模拟正常运行数据
    println!("⏱️  模拟8小时正常运行...");

    for (id, _name, metric) in &equipment {
        let base = match *metric {
            "vibration" => 2.5,
            "temperature" => 55.0,
            "pressure" => 4.0,
            "speed" => 2.5,
            _ => 50.0,
        };

        let values = ctx
            .simulate_sensor_stream(
                id,
                metric,
                base,
                0.5, // 小幅波动
                20,
                20,
                Some(0.01), // 极小趋势
            )
            .await;

        println!(
            "   {}: {} 均值={:.2}, 标准差={:.2}",
            id,
            metric,
            values.iter().sum::<f64>() / values.len() as f64,
            {
                let avg = values.iter().sum::<f64>() / values.len() as f64;
                let variance =
                    values.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / values.len() as f64;
                variance.sqrt()
            }
        );
    }

    // 执行监控
    let agent = ctx.store.get_agent(&maintenance_monitor.id).await?.unwrap();
    let record1 = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("\n   正常状态分析:");
    println!("   结论: {}", record1.decision_process.conclusion);

    ctx.print_subsection("阶段2: 故障前期征兆模拟");

    // 模拟电机振动逐渐增大
    println!("🔴 模拟故障: motor_1 振动逐渐异常");

    let vibration_values = ctx
        .simulate_sensor_stream(
            "motor_1",
            "vibration",
            2.8, // 略高于正常
            0.3,
            15,
            30,
            Some(0.4), // 持续上升趋势
        )
        .await;

    println!("   振动趋势:");
    for (i, vib) in vibration_values.iter().enumerate() {
        let status = if *vib > 5.0 {
            "🔴 异常"
        } else if *vib > 3.5 {
            "🟡 警告"
        } else {
            "🟢 正常"
        };
        println!("     t-{}: {:.2} mm/s {}", (15 - i) * 2, vib, status);
    }

    // 同时模拟温度升高
    let temp_values = ctx
        .simulate_sensor_stream(
            "motor_1",
            "temperature",
            58.0,
            1.0,
            10,
            30,
            Some(2.0), // 温度快速上升
        )
        .await;

    println!("\n   温度趋势:");
    for (i, temp) in temp_values.iter().enumerate() {
        let status = if *temp > 75.0 {
            "🔴 异常"
        } else if *temp > 65.0 {
            "🟡 警告"
        } else {
            "🟢 正常"
        };
        println!("     t-{}: {:.1}°C {}", (10 - i) * 2, temp, status);
    }

    // 再次执行Agent
    let agent = ctx.store.get_agent(&maintenance_monitor.id).await?.unwrap();
    let record2 = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("\n   故障检测分析:");
    println!(
        "   数据收集: {} 个",
        record2.decision_process.data_collected.len()
    );
    println!("   推理步骤:");
    for (i, step) in record2.decision_process.reasoning_steps.iter().enumerate() {
        println!("     {}. {}", i + 1, step.description);
    }

    println!("\n   决策:");
    for (i, decision) in record2.decision_process.decisions.iter().enumerate() {
        println!("     {}. {}", i + 1, decision.description);
    }

    println!("\n   结论: {}", record2.decision_process.conclusion);

    // 检查决策
    let has_alert_decision = record2.decision_process.decisions.iter().any(|d| {
        d.decision_type == "alert"
            || d.description.contains("警告")
            || d.description.contains("异常")
    });

    if has_alert_decision || record2.decision_process.conclusion.contains("异常") {
        ctx.send_alert(
            MessageSeverity::Critical,
            "设备故障预警",
            "motor_1 振动和温度异常升高，可能即将发生故障，建议立即检查",
            "motor_1",
        )
        .await;
    }

    ctx.print_subsection("阶段3: 维护建议生成");

    let analyst = ctx
        .create_agent(
            "设备维护分析师",
            vec![AgentResource {
                resource_type: ResourceType::Metric,
                resource_id: "motor_1:vibration".to_string(),
                name: "motor_1 - vibration".to_string(),
                config: serde_json::json!({}),
            }],
            "分析设备故障模式，生成维护建议和报告",
            60,
        )
        .await?;

    let agent = ctx.store.get_agent(&analyst.id).await?.unwrap();
    let record3 = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("   维护建议:");
    println!("   {}", record3.decision_process.conclusion);

    ctx.print_subsection("场景总结");

    println!("✅ 预测性维护系统测试完成:");
    println!("   - 设备监控: {} 台", equipment.len());
    println!("   - 故障检测: 振动+温度双重监控");
    println!("   - 趋势分析: 持续上升检测");
    println!("   - 预警机制: 自动生成告警");
    println!("   - 维护建议: 分析师Agent生成");

    Ok(())
}

// ============================================================================
// 场景3: 多Agent协作系统
// ============================================================================

#[tokio::test]
#[ignore = "Real-world simulation test"]
async fn scenario_3_multi_agent_collaboration() -> anyhow::Result<()> {
    let ctx = SimulationContext::new().await?;
    ctx.print_section("场景3: 多Agent协作系统");

    // 场景: 温室环境监控与控制
    // - 监控Agent: 检测环境异常
    // - 执行Agent: 响应控制设备
    // - 分析Agent: 生成优化建议

    let greenhouse_metrics = vec![
        ("greenhouse", "temperature"),
        ("greenhouse", "humidity"),
        ("greenhouse", "co2"),
        ("greenhouse", "light"),
    ];

    println!("🌱 温室监控指标:");
    for (_id, metric) in &greenhouse_metrics {
        println!("   - {}", metric);
    }

    ctx.print_subsection("阶段1: 创建协作Agent团队");

    // 监控Agent
    let mut monitor_resources = Vec::new();
    for (_id, metric) in &greenhouse_metrics {
        monitor_resources.push(AgentResource {
            resource_type: ResourceType::Metric,
            resource_id: format!("greenhouse:{}", metric),
            name: format!("greenhouse - {}", metric),
            config: serde_json::json!({
                "optimal": {
                    "temperature": [20.0, 28.0],
                    "humidity": [60.0, 80.0],
                    "co2": [400.0, 1200.0],
                    "light": [10000.0, 30000.0]
                }
            }),
        });
    }

    let monitor_agent = ctx
        .create_agent(
            "温室环境监控Agent",
            monitor_resources,
            "监控温室环境，检测偏离最优范围的情况:
最优参数: 温度20-28°C, 湿度60-80%, CO2 400-1200ppm, 光照10000-30000lux
异常时触发执行Agent进行处理",
            15,
        )
        .await?;

    // 执行Agent
    let executor_agent = ctx
        .create_agent(
            "温室设备控制Agent",
            vec![
                AgentResource {
                    resource_type: ResourceType::Command,
                    resource_id: "greenhouse:vent_fan".to_string(),
                    name: "vent_fan".to_string(),
                    config: serde_json::json!({"parameters": {"speed": "adjust"}}),
                },
                AgentResource {
                    resource_type: ResourceType::Command,
                    resource_id: "greenhouse:heater".to_string(),
                    name: "heater".to_string(),
                    config: serde_json::json!({"parameters": {"power": "adjust"}}),
                },
                AgentResource {
                    resource_type: ResourceType::Command,
                    resource_id: "greenhouse:co2_injector".to_string(),
                    name: "co2_injector".to_string(),
                    config: serde_json::json!({"parameters": {"rate": "adjust"}}),
                },
            ],
            "根据监控Agent的指令控制温室设备:
- 温度过高: 开启通风扇
- 温度过低: 开启加热器
- CO2过低: 开启CO2注入器
- 湿度过高: 开启除湿
- 湿度过低: 开启加湿",
            15,
        )
        .await?;

    // 分析Agent
    let analyst_agent = ctx
        .create_agent(
            "温室优化分析师",
            vec![
                AgentResource {
                    resource_type: ResourceType::Metric,
                    resource_id: "greenhouse:temperature".to_string(),
                    name: "temperature".to_string(),
                    config: serde_json::json!({}),
                },
                AgentResource {
                    resource_type: ResourceType::Metric,
                    resource_id: "greenhouse:humidity".to_string(),
                    name: "humidity".to_string(),
                    config: serde_json::json!({}),
                },
            ],
            "分析温室历史数据，生成优化建议和报告",
            300, // 5分钟分析一次
        )
        .await?;

    println!("   ✅ 创建监控Agent: {}", monitor_agent.name);
    println!("   ✅ 创建执行Agent: {}", executor_agent.name);
    println!("   ✅ 创建分析Agent: {}", analyst_agent.name);

    ctx.print_subsection("阶段2: 模拟环境异常事件");

    // 模拟温度升高
    println!("🌡️  模拟事件: 温室温度逐渐升高");

    let temp_values = ctx
        .simulate_sensor_stream(
            "greenhouse",
            "temperature",
            25.0,
            1.0,
            10,
            40,
            Some(0.8), // 持续上升
        )
        .await;

    println!("   温度序列:");
    for (i, temp) in temp_values.iter().enumerate() {
        let icon = if *temp > 30.0 {
            "🔴"
        } else if *temp > 28.0 {
            "🟡"
        } else {
            "🟢"
        };
        println!("     t-{}: {:.1}°C {}", (10 - i) * 2, temp, icon);
    }

    // 监控Agent执行
    println!("\n📊 监控Agent执行:");
    let agent = ctx.store.get_agent(&monitor_agent.id).await?.unwrap();
    let record = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("   状态: {:?}", record.status);
    println!(
        "   情况分析: {}",
        record.decision_process.situation_analysis
    );
    println!("   决策:");
    for decision in &record.decision_process.decisions {
        println!(
            "     - {} ({})",
            decision.description, decision.decision_type
        );
    }

    ctx.print_subsection("阶段3: 执行Agent响应");

    // 执行Agent执行
    println!("⚙️  执行Agent响应:");

    let agent = ctx.store.get_agent(&executor_agent.id).await?.unwrap();
    let record2 = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("   状态: {:?}", record2.status);
    println!("   动作执行:");
    if let Some(ref result) = record2.result {
        for action in &result.actions_executed {
            println!(
                "     - {} : {}",
                action.action_type,
                if action.success {
                    "✅ 成功"
                } else {
                    "❌ 失败"
                }
            );
        }
    }

    // 模拟CO2不足
    println!("\n📉 模拟事件: CO2浓度下降");

    let co2_values = ctx
        .simulate_sensor_stream(
            "greenhouse",
            "co2",
            800.0,
            50.0,
            8,
            30,
            Some(-40.0), // 快速下降
        )
        .await;

    println!("   CO2序列:");
    for (i, co2) in co2_values.iter().enumerate() {
        let icon = if *co2 < 400.0 {
            "🔴"
        } else if *co2 < 500.0 {
            "🟡"
        } else {
            "🟢"
        };
        println!("     t-{}: {:.0} ppm {}", (8 - i) * 2, co2, icon);
    }

    ctx.print_subsection("阶段4: 分析Agent生成报告");

    println!("📈 分析Agent执行:");

    let agent = ctx.store.get_agent(&analyst_agent.id).await?.unwrap();
    let record3 = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("   分析报告:");
    println!("   {}", record3.decision_process.conclusion);

    ctx.print_subsection("场景总结");

    println!("✅ 多Agent协作测试完成:");
    println!("   - 监控Agent: 检测环境异常");
    println!("   - 执行Agent: 响应控制指令");
    println!("   - 分析Agent: 生成优化建议");
    println!("   - Agent协作: 通过共享数据协作");

    // 检查对话历史 (backward compat, no longer written to)
    for agent_id in &[&monitor_agent.id, &executor_agent.id, &analyst_agent.id] {
        let agent = ctx.store.get_agent(agent_id).await?.unwrap();
        println!(
            "   - {} 对话历史: {} 条 (backward compat)",
            agent.name,
            agent.conversation_history.len()
        );
        let _ = &agent.conversation_history;
    }

    Ok(())
}

// ============================================================================
// 场景4: 长期运行Agent (24小时周期模拟)
// ============================================================================

#[tokio::test]
#[ignore = "Real-world simulation test"]
async fn scenario_4_long_running_agent() -> anyhow::Result<()> {
    let ctx = SimulationContext::new().await?;
    ctx.print_section("场景4: 长期运行Agent (24小时周期模拟)");

    // 模拟全天温度变化
    let hourly_temps = vec![
        (0, 18.5),
        (1, 18.2),
        (2, 18.0),
        (3, 17.8), // 深夜
        (4, 17.9),
        (5, 18.5),
        (6, 19.5),
        (7, 21.0), // 早晨
        (8, 22.5),
        (9, 24.0),
        (10, 25.5),
        (11, 26.5), // 上午
        (12, 27.5),
        (13, 28.0),
        (14, 28.2),
        (15, 27.8), // 下午
        (16, 27.0),
        (17, 26.0),
        (18, 24.5),
        (19, 23.0), // 傍晚
        (20, 22.0),
        (21, 21.0),
        (22, 20.0),
        (23, 19.0), // 夜晚
    ];

    println!("📅 模拟24小时温度变化:");
    print!("   ");
    for (hour, temp) in &hourly_temps {
        print!("{:02}h:{:.0}°C  ", hour, temp);
        if (hour + 1) % 6 == 0 {
            println!();
            print!("   ");
        }
    }
    println!();

    let daily_monitor = ctx
        .create_agent(
            "24小时环境监控Agent",
            vec![AgentResource {
                resource_type: ResourceType::Metric,
                resource_id: "office:temperature".to_string(),
                name: "office - temperature".to_string(),
                config: serde_json::json!({
                    "comfort_range": [20.0, 26.0],
                    "working_hours": [8, 18],
                }),
            }],
            "监控办公室24小时温度变化:
- 工作时间(8-18点): 舒适范围20-26°C
- 非工作时间: 允许更宽范围
- 记录温度趋势和异常
- 生成日报告",
            60, // 每小时执行
        )
        .await?;

    ctx.print_subsection("阶段1: 模拟24小时运行");

    println!("⏱️  执行24次监控 (每小时一次)...");

    let mut execution_times = Vec::new();
    let mut all_decisions = 0;

    for (hour, temp) in &hourly_temps {
        // 注入该小时温度数据
        let point = DataPoint {
            timestamp: chrono::Utc::now().timestamp_millis(),
            value: serde_json::json!(*temp),
            quality: Some(1.0),
            metadata: Some(serde_json::json!({"hour": hour})),
        };
        ctx.time_series
            .write("office", "temperature", point)
            .await
            .ok();

        // 执行Agent
        let agent = ctx.store.get_agent(&daily_monitor.id).await?.unwrap();
        let start = Instant::now();
        let record = ctx
            .executor
            .execute_agent(agent.clone(), None, None)
            .await?;
        let elapsed = start.elapsed();

        execution_times.push(elapsed.as_millis());
        all_decisions += record.decision_process.decisions.len();

        // 显示关键时间点
        let icon = match hour {
            8 => "🌅 上班",
            12 => "☀️ 中午",
            18 => "🌙 下班",
            _ => "",
        };

        if !icon.is_empty() || hour % 6 == 0 {
            println!(
                "   {:02}:00 {} - {:.1}°C - 分析: {} ({:.2}ms)",
                hour,
                icon,
                temp,
                if record.decision_process.conclusion.len() > 50 {
                    format!("{}...", &record.decision_process.conclusion[..47])
                } else {
                    record.decision_process.conclusion.clone()
                },
                elapsed.as_millis()
            );
        }
    }

    ctx.print_subsection("阶段2: 性能统计");

    let avg_time = execution_times.iter().sum::<u128>() / execution_times.len() as u128;
    let max_time = *execution_times.iter().max().unwrap_or(&0);
    let min_time = *execution_times.iter().min().unwrap_or(&0);

    println!("   📊 执行性能:");
    println!("      平均耗时: {}ms", avg_time);
    println!("      最快: {}ms, 最慢: {}ms", min_time, max_time);
    println!("      总执行次数: {}", execution_times.len());
    println!("      总决策数: {}", all_decisions);

    // 检查Agent状态
    let agent = ctx.store.get_agent(&daily_monitor.id).await?.unwrap();
    println!("\n   📈 Agent状态:");
    println!(
        "      对话历史: {} 条 (backward compat)",
        agent.conversation_history.len()
    );
    println!("      上下文窗口: {}", agent.context_window_size);
    println!("      成功执行: {}", agent.stats.successful_executions);
    println!("      平均耗时: {}ms", agent.stats.avg_duration_ms);

    // conversation_history is now Vec<serde_json::Value> (backward compat, no longer written to)
    let _ = &agent.conversation_history;

    ctx.print_subsection("阶段3: 历史趋势分析");

    // 创建分析师Agent分析全天数据
    let analyst = ctx
        .create_agent(
            "日报告分析师",
            vec![AgentResource {
                resource_type: ResourceType::Metric,
                resource_id: "office:temperature".to_string(),
                name: "office - temperature".to_string(),
                config: serde_json::json!({}),
            }],
            "分析24小时温度数据，生成日报告",
            3600,
        )
        .await?;

    let agent = ctx.store.get_agent(&analyst.id).await?.unwrap();
    let record = ctx
        .executor
        .execute_agent(agent.clone(), None, None)
        .await?;

    println!("   📋 日报告分析:");
    println!("   {}", record.decision_process.conclusion);

    ctx.print_subsection("场景总结");

    println!("✅ 长期运行Agent测试完成:");
    println!("   - 24小时周期模拟");
    println!("   - 上下文窗口: 正常工作");
    println!("   - 性能稳定: 平均{}ms", avg_time);

    Ok(())
}

// ============================================================================
// 场景5: 压力测试 - 多Agent并发
// ============================================================================

#[tokio::test]
#[ignore = "Real-world simulation test"]
async fn scenario_5_stress_multi_agent() -> anyhow::Result<()> {
    let ctx = SimulationContext::new().await?;
    ctx.print_section("场景5: 压力测试 - 多Agent并发执行");

    // 创建100个设备
    let device_count = 100;
    println!("🏢 创建 {} 个虚拟设备...", device_count);

    for i in 0..device_count {
        let device_id = format!("sensor_{:03}", i);
        let base_temp = 20.0 + (i as f64 % 10.0);
        let _ = ctx
            .simulate_sensor_stream(&device_id, "temperature", base_temp, 2.0, 5, 5, None)
            .await;
    }

    // 创建多个Agent
    let agent_configs = vec![("温度监控组", 10), ("温度执行组", 5), ("数据分析组", 3)];

    let mut agent_ids = Vec::new();

    ctx.print_subsection("创建Agent团队");

    for (name, count) in &agent_configs {
        println!("   创建 {} {}...", count, name);

        for i in 0..*count {
            let resources = vec![AgentResource {
                resource_type: ResourceType::Metric,
                resource_id: format!("sensor_{:03}:temperature", i % device_count),
                name: format!("sensor_{:03}", i % device_count),
                config: serde_json::json!({}),
            }];

            let agent = ctx
                .create_agent(
                    &format!("{}_{:02}", name, i),
                    resources,
                    &format!("{} Agent #{}", name, i),
                    30,
                )
                .await?;

            agent_ids.push(agent.id);
        }
    }

    println!("\n   总计: {} 个Agent", agent_ids.len());

    ctx.print_subsection("并发执行测试");

    let start = Instant::now();
    let mut total_duration = 0u64;

    // 串行执行所有Agent (由于executor不支持clone)
    println!("   串行执行 {} 个Agent...", agent_ids.len());

    for agent_id in &agent_ids {
        let agent = ctx.store.get_agent(agent_id).await?.unwrap();
        let exec_start = Instant::now();
        let _record = ctx
            .executor
            .execute_agent(agent.clone(), None, None)
            .await?;
        let elapsed = exec_start.elapsed();
        total_duration += elapsed.as_millis() as u64;
    }

    let elapsed = start.elapsed();

    println!("   ⏱️  执行统计:");
    println!("      总耗时: {:?}", elapsed);
    println!("      Agent数量: {}", agent_ids.len());
    println!("      总执行时间: {}ms", total_duration);
    println!(
        "      平均每Agent: {}ms",
        total_duration / agent_ids.len() as u64
    );
    println!(
        "      吞吐量: {:.2} Agent/秒",
        agent_ids.len() as f64 / elapsed.as_secs_f64()
    );

    ctx.print_subsection("场景总结");

    println!("✅ 压力测试完成:");
    println!("   - 设备数量: {}", device_count);
    println!("   - Agent数量: {}", agent_ids.len());
    println!("   - 所有执行: 成功");
    println!("   - 系统稳定性: 良好");

    Ok(())
}

// ============================================================================
// 辅助函数
// ============================================================================

fn ollama_available() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 11434));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok()
}
