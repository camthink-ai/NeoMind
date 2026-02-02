//! Test data generation handler.
//!
//! Provides endpoints to generate sample data for development/testing.

use axum::{extract::State};

use edge_ai_messages::{Message, MessageSeverity};
use edge_ai_core::event::{MetricValue, NeoTalkEvent};

use super::{ServerState, common::{HandlerResult, ok}};

#[derive(Debug, serde::Serialize)]
pub struct TestDataSummary {
    pub messages_created: usize,
    pub events_published: usize,
    pub message: String,
}

/// Generate test messages (alerts endpoint redirected to messages).
pub async fn generate_test_alerts_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let messages = generate_test_messages();
    let mut created = 0;

    for message in messages {
        match state.message_manager.create_message(message).await {
            Ok(_) => created += 1,
            Err(_) => {}, // Ignore duplicates
        }
    }

    ok(serde_json::json!({
        "messages_created": created,
        "message": format!("Created {} test messages", created)
    }))
}

/// Publish test events.
pub async fn generate_test_events_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let events = generate_test_events();
    let mut published = 0;

    if let Some(event_bus) = &state.event_bus {
        for event in events {
            event_bus.publish(event).await;
            published += 1;
        }
    }

    ok(serde_json::json!({
        "events_published": published,
        "message": format!("Published {} test events", published)
    }))
}

/// Generate all test data (messages + events).
pub async fn generate_test_data_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let mut summary = TestDataSummary {
        messages_created: 0,
        events_published: 0,
        message: String::new(),
    };

    // Create messages
    let messages = generate_test_messages();
    for message in messages {
        if state.message_manager.create_message(message).await.is_ok() {
            summary.messages_created += 1;
        }
    }

    // Publish events
    if let Some(event_bus) = &state.event_bus {
        let events = generate_test_events();
        for event in events {
            event_bus.publish(event).await;
            summary.events_published += 1;
        }
    }

    summary.message = format!(
        "Generated {} messages and {} events",
        summary.messages_created,
        summary.events_published
    );

    ok(serde_json::to_value(&summary).unwrap())
}

fn generate_test_messages() -> Vec<Message> {
    let mut messages = Vec::new();

    // Emergency messages
    messages.push(Message::new(
        "alert",
        MessageSeverity::Emergency,
        "烟雾检测".to_string(),
        "厨房传感器检测到烟雾，请立即确认！".to_string(),
        "sensor/kitchen".to_string(),
    ));

    messages.push(Message::new(
        "alert",
        MessageSeverity::Emergency,
        "漏水警报".to_string(),
        "地下室检测到漏水，水泵已启动".to_string(),
        "sensor/basement".to_string(),
    ));

    // Critical messages
    messages.push(Message::new(
        "alert",
        MessageSeverity::Critical,
        "冰箱温度过高".to_string(),
        "冰箱内部温度达到 8°C，食物可能变质风险".to_string(),
        "sensor/fridge".to_string(),
    ));

    messages.push(Message::new(
        "alert",
        MessageSeverity::Critical,
        "门锁异常".to_string(),
        "前门锁连续 3 次开锁失败，可能存在异常尝试".to_string(),
        "lock/front".to_string(),
    ));

    messages.push(Message::new(
        "alert",
        MessageSeverity::Critical,
        "电池电量低".to_string(),
        "门锁电池电量低于 10%，请及时更换".to_string(),
        "lock/front".to_string(),
    ));

    // Warning messages
    messages.push(Message::new(
        "alert",
        MessageSeverity::Warning,
        "温度偏高".to_string(),
        "客厅温度达到 28°C，超过设定阈值 26°C".to_string(),
        "sensor/living".to_string(),
    ));

    messages.push(Message::new(
        "alert",
        MessageSeverity::Warning,
        "设备离线警告".to_string(),
        "传感器 sensor/garden-01 已超过 5 分钟未上报数据".to_string(),
        "device_monitor".to_string(),
    ));

    messages.push(Message::new(
        "system",
        MessageSeverity::Warning,
        "存储空间不足".to_string(),
        "系统存储空间使用率超过 85%".to_string(),
        "system/monitor".to_string(),
    ));

    // Info messages
    messages.push(Message::new(
        "system",
        MessageSeverity::Info,
        "系统启动完成".to_string(),
        "NeoMind 系统已成功启动，所有服务正常运行".to_string(),
        "system".to_string(),
    ));

    messages.push(Message::new(
        "system",
        MessageSeverity::Info,
        "固件更新可用".to_string(),
        "网关设备有新固件版本 v2.1.0 可用".to_string(),
        "update_manager".to_string(),
    ));

    messages.push(Message::new(
        "business",
        MessageSeverity::Info,
        "场景执行成功".to_string(),
        "「回家模式」场景已自动执行".to_string(),
        "automation".to_string(),
    ));

    messages
}

fn generate_test_events() -> Vec<NeoTalkEvent> {
    let now = chrono::Utc::now().timestamp();
    let mut events = Vec::new();

    // Device online events
    events.push(NeoTalkEvent::DeviceOnline {
        device_id: "sensor/living".to_string(),
        device_type: "sensor".to_string(),
        timestamp: now,
    });

    events.push(NeoTalkEvent::DeviceOnline {
        device_id: "light/bedroom".to_string(),
        device_type: "light".to_string(),
        timestamp: now,
    });

    events.push(NeoTalkEvent::DeviceOnline {
        device_id: "sensor/temp".to_string(),
        device_type: "sensor".to_string(),
        timestamp: now,
    });

    // Device metric events
    events.push(NeoTalkEvent::DeviceMetric {
        device_id: "sensor/temp".to_string(),
        metric: "temperature".to_string(),
        value: MetricValue::Float(25.5),
        timestamp: now,
        quality: None,
    });

    events.push(NeoTalkEvent::DeviceMetric {
        device_id: "sensor/temp".to_string(),
        metric: "humidity".to_string(),
        value: MetricValue::Float(60.0),
        timestamp: now,
        quality: Some(100.0),
    });

    events.push(NeoTalkEvent::DeviceMetric {
        device_id: "sensor/living".to_string(),
        metric: "temperature".to_string(),
        value: MetricValue::Float(28.5),
        timestamp: now,
        quality: Some(95.0),
    });

    // Device command result events
    events.push(NeoTalkEvent::DeviceCommandResult {
        device_id: "light/living".to_string(),
        command: "on".to_string(),
        success: true,
        result: Some(serde_json::json!({"brightness": 80})),
        timestamp: now,
    });

    events.push(NeoTalkEvent::DeviceCommandResult {
        device_id: "lock/front".to_string(),
        command: "lock".to_string(),
        success: true,
        result: Some(serde_json::json!({"status": "locked"})),
        timestamp: now,
    });

    // Message created events (for testing)
    events.push(NeoTalkEvent::MessageCreated {
        message_id: "test-msg-1".to_string(),
        title: "测试消息".to_string(),
        severity: "info".to_string(),
        message: "这是一条测试消息".to_string(),
        timestamp: now,
    });

    events
}
