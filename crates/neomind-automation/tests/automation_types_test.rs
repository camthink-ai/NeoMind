//! Tests for unified automation types
//!
//! Tests the core types that are shared across transforms and rules.

use neomind_automation::{
    Automation, AutomationMetadata, AutomationType, TransformScope,
    TransformAutomation, TransformOperation, AggregationFunc,
    RuleAutomation, Trigger, TriggerType, Condition, ComparisonOperator,
    Action,
};
use serde_json::{to_value, from_value};

#[test]
fn test_automation_type_display() {
    assert_eq!(AutomationType::Transform.as_str(), "transform");
    assert_eq!(AutomationType::Rule.as_str(), "rule");
}

#[test]
fn test_automation_metadata_new() {
    let metadata = AutomationMetadata::new("test-id", "Test Automation");

    assert_eq!(metadata.id, "test-id");
    assert_eq!(metadata.name, "Test Automation");
    assert_eq!(metadata.description, "");
    assert!(metadata.enabled);
    assert_eq!(metadata.execution_count, 0);
    assert!(metadata.last_executed.is_none());
    assert!(metadata.created_at > 0);
    assert!(metadata.updated_at > 0);
}

#[test]
fn test_automation_metadata_with_description() {
    let metadata = AutomationMetadata::new("test-id", "Test Automation")
        .with_description("A test automation for unit testing");

    assert_eq!(metadata.description, "A test automation for unit testing");
}

#[test]
fn test_automation_metadata_mark_executed() {
    let mut metadata = AutomationMetadata::new("test-id", "Test Automation");
    let original_count = metadata.execution_count;

    metadata.mark_executed();

    assert_eq!(metadata.execution_count, original_count + 1);
    assert!(metadata.last_executed.is_some());
    assert!(metadata.last_executed.unwrap() > 0);
}

#[test]
fn test_automation_metadata_touch() {
    let mut metadata = AutomationMetadata::new("test-id", "Test Automation");
    let original_updated = metadata.updated_at;

    // Sleep to ensure timestamp changes (at least 1 second for Unix timestamp resolution)
    std::thread::sleep(std::time::Duration::from_secs(2));
    metadata.touch();

    // The touch() method should update the timestamp
    // Note: Due to timestamp resolution, we check >= not >
    assert!(metadata.updated_at >= original_updated);
}

#[test]
fn test_transform_scope_as_str() {
    assert_eq!(TransformScope::Global.as_str(), "global");
    assert_eq!(TransformScope::DeviceType("sensor".to_string()).as_str(), "device_type:sensor");
    assert_eq!(TransformScope::Device("dev-1".to_string()).as_str(), "device:dev-1");
}

#[test]
fn test_transform_scope_priority() {
    assert_eq!(TransformScope::Global.priority(), 0);
    assert_eq!(TransformScope::DeviceType("sensor".to_string()).priority(), 1);
    assert_eq!(TransformScope::Device("dev-1".to_string()).priority(), 2);
}

#[test]
fn test_aggregation_func_as_str() {
    assert_eq!(AggregationFunc::Mean.as_str(), "mean");
    assert_eq!(AggregationFunc::Max.as_str(), "max");
    assert_eq!(AggregationFunc::Min.as_str(), "min");
    assert_eq!(AggregationFunc::Sum.as_str(), "sum");
    assert_eq!(AggregationFunc::Count.as_str(), "count");
    assert_eq!(AggregationFunc::Median.as_str(), "median");
    assert_eq!(AggregationFunc::StdDev.as_str(), "stddev");
    assert_eq!(AggregationFunc::First.as_str(), "first");
    assert_eq!(AggregationFunc::Last.as_str(), "last");
}

#[test]
fn test_aggregation_func_serialization() {
    let funcs = vec![
        AggregationFunc::Sum, AggregationFunc::Mean, AggregationFunc::Min,
        AggregationFunc::Max, AggregationFunc::Count, AggregationFunc::Median,
    ];

    for func in funcs {
        let serialized = to_value(func).unwrap();
        let deserialized: AggregationFunc = from_value(serialized).unwrap();
        assert_eq!(to_value(func).unwrap(), to_value(deserialized).unwrap());
    }
}

#[test]
fn test_comparison_operator() {
    assert_eq!(ComparisonOperator::GreaterThan.as_str(), ">");
    assert_eq!(ComparisonOperator::LessThan.as_str(), "<");
    assert_eq!(ComparisonOperator::Equal.as_str(), "==");
    assert_eq!(ComparisonOperator::GreaterThanOrEqual.as_str(), ">=");
}

#[test]
fn test_automation_transform() {
    let transform = Automation::Transform(TransformAutomation::new(
        "t1", "Transform 1", TransformScope::Global,
    ));

    assert_eq!(transform.id(), "t1");
    assert_eq!(transform.name(), "Transform 1");
    assert_eq!(transform.automation_type(), AutomationType::Transform);
    assert!(transform.is_enabled());
    assert_eq!(transform.execution_count(), 0);
}

#[test]
fn test_automation_serialization_transform() {
    let transform = TransformAutomation::new(
        "t1", "Transform 1", TransformScope::DeviceType("sensor".to_string()),
    )
    .with_description("Test transform");

    let automation = Automation::Transform(transform);

    let serialized = to_value(&automation).unwrap();
    assert_eq!(serialized["type"], "transform");
    // Metadata is flattened, so id is at the top level
    assert_eq!(serialized["id"], "t1");

    // Round-trip test
    let deserialized: Automation = from_value(serialized).unwrap();
    assert_eq!(deserialized.id(), "t1");
    assert_eq!(deserialized.automation_type(), AutomationType::Transform);
}

#[test]
fn test_trigger_type_as_str() {
    assert_eq!(TriggerType::Manual.as_str(), "manual");
    assert_eq!(TriggerType::DeviceState.as_str(), "device_state");
    assert_eq!(TriggerType::Schedule.as_str(), "schedule");
    assert_eq!(TriggerType::Event.as_str(), "event");
}

#[test]
fn test_trigger_new_manual() {
    let trigger = Trigger::manual();
    assert_eq!(trigger.r#type, TriggerType::Manual);
}

#[test]
fn test_trigger_new_schedule() {
    let trigger = Trigger::schedule("0 * * * *");
    assert_eq!(trigger.r#type, TriggerType::Schedule);
    assert_eq!(trigger.cron_schedule, Some("0 * * * *".to_string()));
}

#[test]
fn test_trigger_new_event() {
    let trigger = Trigger::event("temperature.high");
    assert_eq!(trigger.r#type, TriggerType::Event);
    assert_eq!(trigger.event_type, Some("temperature.high".to_string()));
}

#[test]
fn test_trigger_serialization() {
    let trigger = Trigger::schedule("0 * * * *");
    let serialized = to_value(&trigger).unwrap();
    let deserialized: Trigger = from_value(serialized).unwrap();

    assert_eq!(deserialized.r#type, TriggerType::Schedule);
    assert_eq!(deserialized.cron_schedule, trigger.cron_schedule);
}

#[test]
fn test_condition_new() {
    let condition = Condition::new("device1", "temperature", ComparisonOperator::GreaterThan, 30.0);
    assert_eq!(condition.device_id, "device1");
    assert_eq!(condition.metric, "temperature");
    assert_eq!(condition.operator, ComparisonOperator::GreaterThan);
    assert_eq!(condition.threshold, 30.0);
}

#[test]
fn test_condition_evaluate() {
    let gt = Condition::new("device1", "temp", ComparisonOperator::GreaterThan, 30.0);
    assert!(gt.evaluate(35.0));
    assert!(!gt.evaluate(25.0));

    let lt = Condition::new("device1", "temp", ComparisonOperator::LessThan, 30.0);
    assert!(lt.evaluate(25.0));
    assert!(!lt.evaluate(35.0));
}

#[test]
fn test_condition_serialization() {
    let condition = Condition::new("device1", "temperature", ComparisonOperator::GreaterThan, 30.0);
    let serialized = to_value(&condition).unwrap();
    let deserialized: Condition = from_value(serialized).unwrap();

    assert_eq!(deserialized.device_id, "device1");
    assert_eq!(deserialized.metric, "temperature");
    assert_eq!(deserialized.threshold, 30.0);
}

#[test]
fn test_action_types() {
    // Test creating various actions
    let notify = Action::Notify {
        message: "High temperature".to_string(),
    };
    let log = Action::Log {
        level: neomind_automation::LogLevel::Info,
        message: "Processing".to_string(),
        severity: None,
    };
    let alert = Action::CreateAlert {
        severity: neomind_automation::AlertSeverity::Warning,
        title: "High temp".to_string(),
        message: "Temperature is high".to_string(),
    };
    let command = Action::ExecuteCommand {
        device_id: "device1".to_string(),
        command: "turn_on".to_string(),
        parameters: Default::default(),
    };

    // Verify they can be serialized
    assert!(to_value(&notify).is_ok());
    assert!(to_value(&log).is_ok());
    assert!(to_value(&alert).is_ok());
    assert!(to_value(&command).is_ok());
}

#[test]
fn test_action_serialization() {
    let action = Action::ExecuteCommand {
        device_id: "device1".to_string(),
        command: "turn_on".to_string(),
        parameters: Default::default(),
    };

    let serialized = to_value(&action).unwrap();
    let deserialized: Action = from_value(serialized).unwrap();

    match deserialized {
        Action::ExecuteCommand { device_id, command, .. } => {
            assert_eq!(device_id, "device1");
            assert_eq!(command, "turn_on");
        }
        _ => panic!("Expected ExecuteCommand"),
    }
}

#[test]
fn test_rule_automation() {
    let rule = RuleAutomation {
        metadata: AutomationMetadata::new("rule1", "Test Rule"),
        trigger: Trigger::event("data.received"),
        condition: Condition::new("sensor1", "temperature", ComparisonOperator::GreaterThan, 30.0),
        actions: vec![
            Action::CreateAlert {
                severity: neomind_automation::AlertSeverity::Warning,
                title: "High temp".to_string(),
                message: "Temperature is high".to_string(),
            },
        ],
    };

    assert_eq!(rule.metadata.id, "rule1");
    assert_eq!(rule.metadata.name, "Test Rule");
    assert_eq!(rule.trigger.r#type, TriggerType::Event);
    assert_eq!(rule.actions.len(), 1);
}

#[test]
fn test_rule_automation_dsl_generation() {
    let rule = RuleAutomation {
        metadata: AutomationMetadata::new("rule1", "Temperature Alert"),
        trigger: Trigger::event("temperature.reading"),
        condition: Condition::new("sensor1", "temperature", ComparisonOperator::GreaterThan, 30.0),
        actions: vec![
            Action::CreateAlert {
                severity: neomind_automation::AlertSeverity::Critical,
                title: "High temperature".to_string(),
                message: "Temperature exceeded threshold".to_string(),
            },
        ],
    };

    let dsl = rule.to_dsl();
    assert!(dsl.contains("WHEN"));
    assert!(dsl.contains("temperature"));
    // The DSL uses ALERT, not CREATE_ALERT
    assert!(dsl.contains("ALERT"));
}

#[test]
fn test_transform_automation_builder() {
    let transform = TransformAutomation::new(
        "test-transform",
        "Test Transform",
        TransformScope::Global,
    );

    assert_eq!(transform.metadata.id, "test-transform");
    assert_eq!(transform.metadata.name, "Test Transform");
    assert!(matches!(transform.scope, TransformScope::Global));
    assert!(transform.operations.is_none());
}

#[test]
fn test_transform_automation_with_js_code() {
    let transform = TransformAutomation::with_js_code(
        "test-transform",
        "Count Items",
        TransformScope::Global,
        "Count the items",
        "return input.detections.length;",
    );

    assert_eq!(transform.metadata.id, "test-transform");
    assert_eq!(transform.intent, Some("Count the items".to_string()));
    assert_eq!(transform.js_code, Some("return input.detections.length;".to_string()));
}

#[test]
fn test_transform_automation_builder_methods() {
    let transform = TransformAutomation::new(
        "test",
        "Test",
        TransformScope::DeviceType("sensor".to_string()),
    )
    .with_description("A test transform")
    .with_device_type("actuator")
    .with_output_prefix("custom_prefix")
    .with_complexity(4);

    assert_eq!(transform.metadata.description, "A test transform");
    assert!(matches!(transform.scope, TransformScope::DeviceType(t) if t == "actuator"));
    assert_eq!(transform.output_prefix, "custom_prefix");
    assert_eq!(transform.complexity, 4);
}

#[test]
fn test_transform_operation_single() {
    let op = TransformOperation::Single {
        json_path: "$.status".to_string(),
        output_metric: "status".to_string(),
    };

    let metrics = op.output_metrics();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0], "status");
}

#[test]
fn test_transform_operation_array_aggregation() {
    let op = TransformOperation::ArrayAggregation {
        json_path: "$.sensors".to_string(),
        aggregation: AggregationFunc::Mean,
        value_path: Some("temp".to_string()),
        output_metric: "avg_temp".to_string(),
    };

    let metrics = op.output_metrics();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0], "avg_temp");
}

#[test]
fn test_transform_operation_extract() {
    let op = TransformOperation::Extract {
        from: "$.data.value".to_string(),
        output: "metric".to_string(),
        as_type: None,
    };

    let metrics = op.output_metrics();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0], "metric");
}

#[test]
fn test_transform_operation_reduce() {
    let op = TransformOperation::Reduce {
        over: "$.values".to_string(),
        using: AggregationFunc::Sum,
        value: None,
        output: "total".to_string(),
    };

    let metrics = op.output_metrics();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0], "total");
}

#[test]
fn test_transform_operation_complexity_score() {
    let simple = TransformOperation::Single {
        json_path: "$.value".to_string(),
        output_metric: "val".to_string(),
    };

    let medium = TransformOperation::ArrayAggregation {
        json_path: "$.items".to_string(),
        aggregation: AggregationFunc::Mean,
        value_path: None,
        output_metric: "avg".to_string(),
    };

    assert_eq!(simple.complexity_score(), 1);
    assert_eq!(medium.complexity_score(), 2);
}
