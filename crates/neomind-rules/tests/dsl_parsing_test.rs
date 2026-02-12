//! Comprehensive tests for the Rules DSL parser.
//!
//! Tests include:
//! - Rule parsing from DSL
//! - Various condition types
//! - Action types
//! - Complex rules
//! - Error handling

use neomind_rules::dsl::{
    AlertSeverity, ComparisonOperator, HttpMethod, LogLevel, RuleAction, RuleCondition,
    RuleDslParser,
};

fn parse_rule(dsl: &str) -> neomind_rules::dsl::ParsedRule {
    RuleDslParser::parse(dsl).expect("Failed to parse rule")
}

#[test]
fn test_parse_temperature_threshold_rule() {
    let dsl = r#"
    rule "High Temperature Alert"
    when temperature > 30
    do
        notify "Temperature is high"
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.name, "High Temperature Alert");
    assert!(rule.description.is_none());
}

#[test]
fn test_parse_rule_with_for_duration() {
    let dsl = r#"
    rule "Persistent High Temperature"
    when temperature > 30
    for 5 minutes
    do
        notify "High temperature for 5 minutes"
    end
    "#;

    let rule = parse_rule(dsl);

    assert!(rule.for_duration.is_some());
    let duration = rule.for_duration.unwrap();
    assert_eq!(duration.as_secs(), 300); // 5 minutes
}

#[test]
fn test_parse_multiple_actions() {
    let dsl = r#"
    rule "Multi-action Rule"
    when temperature > 30
    do
        notify "Temperature is high"
        log info "Temperature exceeded threshold"
        execute device1.turn_on()
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.actions.len(), 3);
}

#[test]
fn test_parse_simple_condition() {
    let dsl = r#"
    rule "Simple Condition"
    when sensor1.temperature > 25
    do
        notify "It's hot!"
    end
    "#;

    let rule = parse_rule(dsl);

    match &rule.condition {
        RuleCondition::Device {
            device_id,
            metric,
            operator,
            threshold,
        } => {
            assert_eq!(device_id, "sensor1");
            assert_eq!(metric, "temperature");
            assert_eq!(*operator, ComparisonOperator::GreaterThan);
            assert_eq!(*threshold, 25.0);
        }
        _ => panic!("Expected Simple condition"),
    }
}

#[test]
fn test_parse_range_condition() {
    let dsl = r#"
    rule "Range Condition"
    when sensor.temperature between 20 and 25
    do
        notify "Temperature is in range"
    end
    "#;

    let rule = parse_rule(dsl);

    match &rule.condition {
        RuleCondition::DeviceRange {
            device_id,
            metric,
            min,
            max,
        } => {
            assert_eq!(device_id, "sensor");
            assert_eq!(metric, "temperature");
            assert_eq!(*min, 20.0);
            assert_eq!(*max, 25.0);
        }
        _ => panic!("Expected Range condition"),
    }
}

#[test]
fn test_parse_and_condition() {
    let dsl = r#"
    rule "And Condition"
    when temperature > 20 and humidity < 50
    do
        notify "Conditions met"
    end
    "#;

    let rule = parse_rule(dsl);

    match &rule.condition {
        RuleCondition::And(conditions) => {
            assert_eq!(conditions.len(), 2);
        }
        _ => panic!("Expected And condition"),
    }
}

#[test]
fn test_parse_or_condition() {
    let dsl = r#"
    rule "Or Condition"
    when temperature > 30 or humidity < 30
    do
        notify "Extreme condition detected"
    end
    "#;

    let rule = parse_rule(dsl);

    match &rule.condition {
        RuleCondition::Or(conditions) => {
            assert_eq!(conditions.len(), 2);
        }
        _ => panic!("Expected Or condition"),
    }
}

#[test]
fn test_parse_not_condition() {
    let dsl = r#"
    rule "Not Condition"
    when not temperature > 30
    do
        notify "Temperature is normal"
    end
    "#;

    let rule = parse_rule(dsl);

    match &rule.condition {
        RuleCondition::Not(_) => (),
        _ => panic!("Expected Not condition"),
    }
}

#[test]
fn test_parse_notify_action() {
    let dsl = r#"
    rule "Notify Action"
    when temperature > 30
    do
        notify "High temperature" [email, sms]
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.actions.len(), 1);
    match &rule.actions[0] {
        RuleAction::Notify { message, channels } => {
            assert_eq!(message, "High temperature");
            assert!(channels.is_some());
            let chans = channels.as_ref().unwrap();
            assert_eq!(chans.len(), 2);
            assert!(chans.contains(&"email".to_string()));
            assert!(chans.contains(&"sms".to_string()));
        }
        _ => panic!("Expected Notify action"),
    }
}

#[test]
fn test_parse_execute_action() {
    let dsl = r#"
    rule "Execute Action"
    when temperature > 30
    do
        execute thermostat1.turn_on(target=22)
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.actions.len(), 1);
    match &rule.actions[0] {
        RuleAction::Execute {
            device_id,
            command,
            params,
        } => {
            assert_eq!(device_id, "thermostat1");
            assert_eq!(command, "turn_on");
            assert!(params.contains_key("target"));
        }
        _ => panic!("Expected Execute action"),
    }
}

#[test]
fn test_parse_log_action() {
    let dsl = r#"
    rule "Log Action"
    when temperature > 30
    do
        log warning "Temperature warning"
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.actions.len(), 1);
    match &rule.actions[0] {
        RuleAction::Log {
            level,
            message,
            severity,
        } => {
            assert_eq!(message, "Temperature warning");
            match level {
                LogLevel::Warning => (),
                _ => panic!("Expected Warning level, got {:?}", level),
            }
            assert!(severity.is_none());
        }
        _ => panic!("Expected Log action"),
    }
}

#[test]
fn test_parse_http_request_action() {
    let dsl = r#"
    rule "HTTP Request Action"
    when temperature > 30
    do
        http post https://api.example.com/alert
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.actions.len(), 1);
    match &rule.actions[0] {
        RuleAction::HttpRequest {
            method,
            url,
            headers: _,
            body,
        } => {
            assert_eq!(*method, HttpMethod::Post);
            assert_eq!(url, "https://api.example.com/alert");
            assert!(body.is_none()); // No body in simple case
        }
        _ => panic!("Expected HttpRequest action"),
    }
}

#[test]
fn test_parse_create_alert_action() {
    let dsl = r#"
    rule "Create Alert Action"
    when temperature > 30
    do
        alert "High Temperature", warning, "Temperature exceeded 30°C"
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.actions.len(), 1);
    match &rule.actions[0] {
        RuleAction::CreateAlert {
            title,
            message,
            severity,
        } => {
            assert_eq!(title, "High Temperature");
            assert_eq!(*severity, AlertSeverity::Warning);
            assert_eq!(message, "Temperature exceeded 30°C");
        }
        _ => panic!("Expected CreateAlert action"),
    }
}

#[test]
fn test_parse_set_action() {
    let dsl = r#"
    rule "Set Action"
    when temperature > 30
    do
        set thermostat.mode = "cool"
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.actions.len(), 1);
    match &rule.actions[0] {
        RuleAction::Set {
            device_id,
            property,
            value,
        } => {
            assert_eq!(device_id, "thermostat");
            assert_eq!(property, "mode");
            assert_eq!(value, &serde_json::json!("cool"));
        }
        _ => panic!("Expected Set action"),
    }
}

#[test]
fn test_parse_with_description() {
    let dsl = r#"
    rule "Test Rule"
    description "This is a test rule for temperature monitoring"
    when temperature > 30
    do
        notify "High temp"
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.name, "Test Rule");
    assert_eq!(rule.description.as_ref().unwrap(), "This is a test rule for temperature monitoring");
}

#[test]
fn test_parse_with_tags() {
    let dsl = r#"
    rule "Tagged Rule"
    tags temperature, alert, urgent
    when temp > 40
    do
        notify "Critical!"
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.name, "Tagged Rule");
    assert_eq!(rule.tags.len(), 3);
    assert!(rule.tags.contains(&"temperature".to_string()));
    assert!(rule.tags.contains(&"alert".to_string()));
    assert!(rule.tags.contains(&"urgent".to_string()));
}

#[test]
fn test_comparison_operators() {
    let operators = [
        (">", ComparisonOperator::GreaterThan),
        ("<", ComparisonOperator::LessThan),
        (">=", ComparisonOperator::GreaterEqual),
        ("<=", ComparisonOperator::LessEqual),
        ("==", ComparisonOperator::Equal),
        ("!=", ComparisonOperator::NotEqual),
    ];

    for (op_str, expected_op) in operators {
        let dsl = &format!(
            r#"
            rule "Test"
            when temperature {} 25
            do
                notify "Condition met"
            end
            "#,
            op_str
        );

        let rule = parse_rule(dsl);

        match &rule.condition {
            RuleCondition::Device { operator, .. } => {
                assert_eq!(*operator, expected_op, "Failed for operator: {}", op_str);
            }
            _ => panic!("Expected Simple condition for operator: {}", op_str),
        }
    }
}

#[test]
fn test_parse_invalid_rule() {
    let dsl = r#"
    rule "Invalid Rule"
    when temperature >>
    do
        notify "Broken"
    end
    "#;

    let result = RuleDslParser::parse(dsl);
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_rule() {
    let dsl = "";

    let result = RuleDslParser::parse(dsl);
    assert!(result.is_err());
}

#[test]
fn test_get_device_metrics() {
    let dsl = r#"
    rule "Multi-device Rule"
    when sensor1.temp > 30 and sensor2.humidity < 50
    do
        notify "Conditions met"
    end
    "#;

    let rule = parse_rule(dsl);

    let metrics = rule.condition.get_device_metrics();
    assert_eq!(metrics.len(), 2);
    assert!(metrics.contains(&("sensor1".to_string(), "temp".to_string())));
    assert!(metrics.contains(&("sensor2".to_string(), "humidity".to_string())));
}

#[test]
fn test_parse_complex_nested_conditions() {
    let dsl = r#"
    rule "Complex Rule"
    when (temp1 > 30 or temp2 < 20) and humidity > 50
    do
        notify "Complex condition met"
    end
    "#;

    let rule = parse_rule(dsl);

    // Should parse without error
    assert_eq!(rule.name, "Complex Rule");
}

#[test]
fn test_parse_http_methods() {
    let methods = [
        ("get", HttpMethod::Get),
        ("post", HttpMethod::Post),
        ("put", HttpMethod::Put),
        ("delete", HttpMethod::Delete),
        ("patch", HttpMethod::Patch),
    ];

    for (method_str, expected_method) in methods {
        let dsl = &format!(
            r#"
            rule "HTTP Test"
            when temp > 30
            do
                http {} https://api.example.com/test
            end
            "#,
            method_str
        );

        let rule = parse_rule(dsl);

        match &rule.actions[0] {
            RuleAction::HttpRequest { method, .. } => {
                assert_eq!(*method, expected_method, "Failed for method: {}", method_str);
            }
            _ => panic!("Expected HttpRequest action for method: {}", method_str),
        }
    }
}

#[test]
fn test_parse_with_all_time_units() {
    let time_units = [
        ("10 seconds", 10),
        ("5 minutes", 300),
        ("2 hours", 7200),
    ];

    for (unit_str, expected_secs) in time_units {
        let dsl = &format!(
            r#"
            rule "Time Unit Test"
            when temp > 30
            for {}
            do
                notify "Test"
            end
            "#,
            unit_str
        );

        let rule = parse_rule(dsl);

        assert!(rule.for_duration.is_some());
        let duration = rule.for_duration.unwrap();
        assert_eq!(duration.as_secs(), expected_secs as u64, "Failed for unit: {}", unit_str);
    }
}

#[test]
fn test_parse_notify_with_channels() {
    let dsl = r#"
    rule "Notify Channels"
    when temp > 30
    do
        notify "Alert" [webhook, email, mobile]
    end
    "#;

    let rule = parse_rule(dsl);

    match &rule.actions[0] {
        RuleAction::Notify { channels, .. } => {
            assert!(channels.is_some());
            let chans = channels.as_ref().unwrap();
            assert_eq!(chans.len(), 3);
        }
        _ => panic!("Expected Notify action with channels"),
    }
}

#[test]
fn test_parse_execute_with_params() {
    let dsl = r#"
    rule "Execute with Params"
    when temp > 30
    do
        execute thermostat.set_temperature(target=22, mode="cool")
    end
    "#;

    let rule = parse_rule(dsl);

    match &rule.actions[0] {
        RuleAction::Execute { params, .. } => {
            assert_eq!(params.len(), 2);
            assert!(params.contains_key("target"));
            assert!(params.contains_key("mode"));
        }
        _ => panic!("Expected Execute action"),
    }
}

#[test]
fn test_parse_multiple_actions_same_type() {
    let dsl = r#"
    rule "Multiple Same Actions"
    when temp > 30
    do
        notify "Alert 1"
        notify "Alert 2"
        notify "Alert 3"
    end
    "#;

    let rule = parse_rule(dsl);

    assert_eq!(rule.actions.len(), 3);
    for action in &rule.actions {
        match action {
            RuleAction::Notify { .. } => (),
            _ => panic!("Expected all Notify actions"),
        }
    }
}

#[test]
fn test_parse_delay_action() {
    let dsl = r#"
    rule "Delay Action"
    when temp > 30
    do
        delay 5 seconds
    end
    "#;

    let rule = parse_rule(dsl);

    match &rule.actions[0] {
        RuleAction::Delay { duration } => {
            assert_eq!(duration.as_secs(), 5);
        }
        _ => panic!("Expected Delay action"),
    }
}
