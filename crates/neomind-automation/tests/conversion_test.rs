//! Tests for Automation conversion utilities

use neomind_automation::{
    Automation, TransformAutomation, TransformScope,
    RuleAutomation, Trigger, AutomationType, AutomationMetadata,
    Condition, ComparisonOperator, Action, AlertSeverity, LogLevel,
};
use serde_json::{to_value, from_value};
use std::collections::HashMap;

#[test]
fn test_automation_get_type() {
    let transform = Automation::Transform(TransformAutomation::new(
        "t1", "Transform 1", TransformScope::Global,
    ));

    let rule = Automation::Rule(RuleAutomation {
        metadata: AutomationMetadata::new("r1", "Rule 1"),
        trigger: Trigger::schedule("* * * * *"),
        condition: Condition::default(),
        actions: vec![],
    });

    assert_eq!(transform.automation_type(), AutomationType::Transform);
    assert_eq!(rule.automation_type(), AutomationType::Rule);
}

#[test]
fn test_automation_complexity_score() {
    let simple_rule = Automation::Rule(RuleAutomation {
        metadata: AutomationMetadata::new("r1", "Simple Rule"),
        trigger: Trigger::manual(),
        condition: Condition::default(),
        actions: vec![],
    });

    let simple_transform = Automation::Transform(TransformAutomation::new(
        "t1", "Simple Transform", TransformScope::Global,
    ));

    assert_eq!(simple_rule.complexity_score(), 1);
    // TransformAutomation has default complexity of 2
    assert_eq!(simple_transform.complexity_score(), 2);
}

#[test]
fn test_automation_is_enabled() {
    let mut transform = TransformAutomation {
        metadata: AutomationMetadata::new("t1", "Transform"),
        scope: TransformScope::Global,
        intent: None,
        js_code: None,
        output_prefix: "transform".to_string(),
        complexity: 2,
        operations: None,
    };
    transform.metadata.enabled = true;

    let mut disabled_transform = TransformAutomation {
        metadata: AutomationMetadata::new("t2", "Disabled Transform"),
        scope: TransformScope::Global,
        intent: None,
        js_code: None,
        output_prefix: "transform".to_string(),
        complexity: 2,
        operations: None,
    };
    disabled_transform.metadata.enabled = false;

    let enabled = Automation::Transform(transform);
    let disabled = Automation::Transform(disabled_transform);

    assert!(enabled.is_enabled());
    assert!(!disabled.is_enabled());
}

#[test]
fn test_automation_execution_count() {
    let mut transform = TransformAutomation {
        metadata: AutomationMetadata::new("t1", "Transform"),
        scope: TransformScope::Global,
        intent: None,
        js_code: None,
        output_prefix: "transform".to_string(),
        complexity: 2,
        operations: None,
    };

    transform.metadata.execution_count = 42;
    let automation = Automation::Transform(transform);

    assert_eq!(automation.execution_count(), 42);
}

#[test]
fn test_automation_last_executed() {
    let mut transform = TransformAutomation {
        metadata: AutomationMetadata::new("t1", "Transform"),
        scope: TransformScope::Global,
        intent: None,
        js_code: None,
        output_prefix: "transform".to_string(),
        complexity: 2,
        operations: None,
    };

    let timestamp = 1234567890;
    transform.metadata.last_executed = Some(timestamp);
    let automation = Automation::Transform(transform);

    assert_eq!(automation.last_executed(), Some(timestamp));
}

#[test]
fn test_automation_round_trip_serialization() {
    let original = Automation::Rule(RuleAutomation {
        metadata: AutomationMetadata::new("rule1", "Test Rule")
            .with_description("Test description"),
        trigger: Trigger::event("data.received"),
        condition: Condition::new("device1", "temp", ComparisonOperator::GreaterThan, 25.0),
        actions: vec![
            Action::CreateAlert {
                severity: AlertSeverity::Warning,
                title: "High temp".to_string(),
                message: "Temperature is too high".to_string(),
            },
        ],
    });

    let serialized = to_value(&original).unwrap();
    let deserialized: Automation = from_value(serialized).unwrap();

    assert_eq!(deserialized.id(), original.id());
    assert_eq!(deserialized.name(), original.name());
    assert_eq!(deserialized.automation_type(), original.automation_type());
}

#[test]
fn test_automation_metadata_serialization() {
    let metadata = AutomationMetadata::new("id123", "Test")
        .with_description("A test automation");

    let serialized = to_value(&metadata).unwrap();
    let deserialized: AutomationMetadata = from_value(serialized).unwrap();

    assert_eq!(deserialized.id, "id123");
    assert_eq!(deserialized.name, "Test");
    assert_eq!(deserialized.description, "A test automation");
    assert!(deserialized.enabled);
}

#[test]
fn test_trigger_serialization() {
    let triggers = vec![
        Trigger::manual(),
        Trigger::schedule("0 * * * *"),
        Trigger::event("temperature.high"),
    ];

    for trigger in triggers {
        let serialized = to_value(&trigger).unwrap();
        let deserialized: Trigger = from_value(serialized.clone()).unwrap();

        assert_eq!(deserialized.r#type, trigger.r#type);
    }
}

#[test]
fn test_action_variants_serialization() {
    let mut parameters = HashMap::new();
    parameters.insert("duration".to_string(), "60".to_string());

    let actions = vec![
        Action::Notify {
            message: "Alert!".to_string(),
        },
        Action::Log {
            message: "Processing".to_string(),
            level: LogLevel::Info,
            severity: None,
        },
        Action::ExecuteCommand {
            device_id: "device1".to_string(),
            command: "turn_on".to_string(),
            parameters,
        },
        Action::CreateAlert {
            severity: AlertSeverity::Warning,
            title: "High temp".to_string(),
            message: "Temperature is high".to_string(),
        },
    ];

    for action in actions {
        let serialized = to_value(&action).unwrap();
        let deserialized: Action = from_value(serialized.clone()).unwrap();

        // Round trip should succeed
        let reserialized = to_value(&deserialized).unwrap();
        assert_eq!(serialized, reserialized);
    }
}

#[test]
fn test_condition_serialization() {
    let condition = Condition::new("device1", "temperature", ComparisonOperator::GreaterThan, 30.0);

    let serialized = to_value(&condition).unwrap();
    let deserialized: Condition = from_value(serialized.clone()).unwrap();

    assert_eq!(deserialized.device_id, "device1");
    assert_eq!(deserialized.metric, "temperature");
    assert_eq!(deserialized.operator, ComparisonOperator::GreaterThan);
    assert_eq!(deserialized.threshold, 30.0);
}

#[test]
fn test_comparison_operator_all() {
    let operators = vec![
        ComparisonOperator::GreaterThan,
        ComparisonOperator::GreaterThanOrEqual,
        ComparisonOperator::LessThan,
        ComparisonOperator::LessThanOrEqual,
        ComparisonOperator::Equal,
        ComparisonOperator::NotEqual,
    ];

    for op in operators {
        let serialized = to_value(&op).unwrap();
        let deserialized: ComparisonOperator = from_value(serialized.clone()).unwrap();

        assert_eq!(serialized, to_value(&deserialized).unwrap());
    }
}

#[test]
fn test_alert_severity_all() {
    let severities = vec![
        AlertSeverity::Info,
        AlertSeverity::Warning,
        AlertSeverity::Critical,
    ];

    for severity in severities {
        let serialized = to_value(&severity).unwrap();
        let deserialized: AlertSeverity = from_value(serialized.clone()).unwrap();

        assert_eq!(serialized, to_value(&deserialized).unwrap());
    }
}

#[test]
fn test_log_level_all() {
    let levels = vec![
        LogLevel::Debug,
        LogLevel::Info,
        LogLevel::Warning,
        LogLevel::Error,
    ];

    for level in levels {
        let serialized = to_value(&level).unwrap();
        let deserialized: LogLevel = from_value(serialized.clone()).unwrap();

        assert_eq!(serialized, to_value(&deserialized).unwrap());
    }
}
