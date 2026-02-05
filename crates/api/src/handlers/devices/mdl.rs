//! MDL (Message Definition Language) generation handlers.

use axum::{Json, extract::State};
use std::collections::BTreeMap;

use edge_ai_devices::{
    mdl::{MetricDataType, MetricValue},
    mdl_format::{
        CommandDefinition, DeviceTypeDefinition, DownlinkConfig, MetricDefinition,
        ParameterDefinition, UplinkConfig,
    },
};

use super::models::GenerateMdlRequest;
use crate::handlers::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

/// Generate MDL from sample data (Plan A: pure parsing, no LLM).
///
/// POST /api/devices/generate-mdl
///
/// This endpoint:
/// 1. Parses sample JSON data 100% reliably
/// 2. Infers data_type from value types
/// 3. Infers unit from field name patterns
/// 4. Generates basic display_name from field names
/// 5. Returns complete MDL JSON for user to edit
pub async fn generate_mdl_handler(
    State(_state): State<ServerState>,
    Json(req): Json<GenerateMdlRequest>,
) -> HandlerResult<DeviceTypeDefinition> {
    // Validate device name
    if req.device_name.trim().is_empty() {
        return Err(ErrorResponse::bad_request("Device name is required"));
    }

    // Generate device_type from device_name (lowercase, replace spaces with underscores)
    let device_type = req
        .device_name
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");

    // Parse and flatten uplink example
    let uplink_metrics = if req.uplink_example.trim().is_empty() {
        vec![]
    } else {
        match serde_json::from_str::<serde_json::Value>(&req.uplink_example) {
            Ok(sample) => {
                let flattened = flatten_json(&sample, "");

                if flattened.is_empty() {
                    // If sample is a primitive value, create a single metric
                    vec![MetricDefinition {
                        name: "value".to_string(),
                        display_name: "Value".to_string(),
                        data_type: infer_data_type(&sample),
                        unit: String::new(),
                        min: None,
                        max: None,
                        required: false,
                    }]
                } else {
                    flattened
                        .into_iter()
                        .map(|(name, value)| MetricDefinition {
                            name: name.clone(),
                            display_name: generate_display_name(&name),
                            data_type: infer_data_type(&value),
                            unit: infer_unit(&name).to_string(),
                            min: None,
                            max: None,
                            required: false,
                        })
                        .collect()
                }
            }
            Err(e) => {
                return Err(ErrorResponse::bad_request(format!(
                    "Invalid uplink example JSON: {}",
                    e
                )));
            }
        }
    };

    // Parse and flatten downlink example
    // Supports multiple JSON objects: {"action": "on"} {"action": "off", "brightness": 50}
    let downlink_commands = if req.downlink_example.trim().is_empty() {
        vec![]
    } else {
        parse_downlink_commands(&req.downlink_example)?
    };

    // Build the complete MDL definition
    let def = DeviceTypeDefinition {
        device_type: device_type.clone(),
        name: req.device_name.clone(),
        description: req.description.clone(),
        categories: vec!["sensor".to_string()],
        mode: edge_ai_devices::mdl_format::DeviceTypeMode::Full,
        uplink: UplinkConfig {
            metrics: uplink_metrics,
            samples: vec![],
        },
        downlink: DownlinkConfig {
            commands: downlink_commands,
        },
    };

    ok(def)
}

/// Flatten a JSON value into a map of dot-separated keys to values.
/// Supports up to 10 levels of nesting, including arrays of objects and arrays of primitives.
fn flatten_json(value: &serde_json::Value, prefix: &str) -> BTreeMap<String, serde_json::Value> {
    let mut result = BTreeMap::new();

    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                let new_key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                // Recursively flatten nested objects and arrays
                if val.is_object() || val.is_array() {
                    result.extend(flatten_json(val, &new_key));
                } else {
                    result.insert(new_key, val.clone());
                }
            }
        }
        serde_json::Value::Array(arr) => {
            // Expand array elements with index notation
            for (i, val) in arr.iter().enumerate() {
                let new_key = if prefix.is_empty() {
                    i.to_string()
                } else {
                    format!("{}.{}", prefix, i)
                };

                if val.is_object() || val.is_array() {
                    result.extend(flatten_json(val, &new_key));
                } else {
                    result.insert(new_key, val.clone());
                }
            }
        }
        _ => {
            if !prefix.is_empty() {
                result.insert(prefix.to_string(), value.clone());
            }
        }
    }

    result
}

/// Infer data type from a JSON value.
fn infer_data_type(value: &serde_json::Value) -> MetricDataType {
    match value {
        serde_json::Value::Number(n) => {
            if n.as_i64().is_some() {
                MetricDataType::Integer
            } else {
                MetricDataType::Float
            }
        }
        serde_json::Value::String(_) => MetricDataType::String,
        serde_json::Value::Bool(_) => MetricDataType::Boolean,
        serde_json::Value::Null => MetricDataType::String,
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => MetricDataType::String,
    }
}

/// Convert a serde_json::Value to MetricValue for default values.
fn json_to_metric_value(value: &serde_json::Value) -> Option<MetricValue> {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(MetricValue::Integer(i))
            } else { n.as_f64().map(MetricValue::Float) }
        }
        serde_json::Value::String(s) => Some(MetricValue::String(s.clone())),
        serde_json::Value::Bool(b) => Some(MetricValue::Boolean(*b)),
        serde_json::Value::Null => Some(MetricValue::Null),
        _ => None,
    }
}

/// Parse downlink commands from example JSON.
/// Supports both single JSON object and multiple concatenated JSON objects.
/// Examples:
/// - Single: {"action": "turn_on", "brightness": 100}
/// - Multiple: {"action": "turn_on"} {"action": "turn_off"} {"action": "set_brightness", "value": 50}
fn parse_downlink_commands(example: &str) -> Result<Vec<CommandDefinition>, ErrorResponse> {
    let trimmed = example.trim();

    // Try parsing as a single JSON object/array first
    if let Ok(single) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(commands_from_json_value(single));
    }

    // Try parsing multiple concatenated JSON objects
    // This handles formats like: {"a": 1} {"b": 2} {"c": 3}
    let mut commands = Vec::new();
    let mut remaining = trimmed;
    let mut index = 0;

    while !remaining.trim().is_empty() {
        match parse_one_json(remaining) {
            Ok((json, rest)) => {
                commands.extend(commands_from_json_value(json));
                remaining = rest;
                index += 1;

                // Safety limit to prevent infinite loops
                if index > 100 {
                    return Err(ErrorResponse::bad_request(
                        "Too many JSON objects (max 100)",
                    ));
                }
            }
            Err(e) => {
                return Err(ErrorResponse::bad_request(format!(
                    "Failed to parse JSON object #{}: {}. Remaining: '{}'",
                    index + 1,
                    e,
                    remaining.chars().take(50).collect::<String>()
                )));
            }
        }
    }

    if commands.is_empty() {
        return Err(ErrorResponse::bad_request(
            "No valid commands found in downlink example",
        ));
    }

    Ok(commands)
}

/// Parse a single JSON object from the beginning of a string.
/// Returns the parsed value and the remaining unparsed string.
fn parse_one_json(input: &str) -> Result<(serde_json::Value, &str), String> {
    let mut chars = input.chars().peekable();
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut start_idx = None;
    let mut end_idx = 0;

    // Skip leading whitespace and count how many we skipped
    let leading_ws = input.chars().take_while(|c| c.is_whitespace()).count();

    // Skip the same whitespace in the iterator
    for _ in 0..leading_ws {
        chars.next();
    }

    for (i, c) in chars.enumerate() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match c {
            '\\' if in_string => {
                escape_next = true;
            }
            '"' if !escape_next => {
                in_string = !in_string;
            }
            '{' | '[' if !in_string => {
                if depth == 0 {
                    // Store the actual position in the original string
                    start_idx = Some(leading_ws + i);
                }
                depth += 1;
            }
            '}' | ']' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    end_idx = leading_ws + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    let start = start_idx.ok_or("No JSON object found")?;
    let end = if end_idx == 0 {
        return Err("Unclosed JSON object".to_string());
    } else {
        end_idx
    };

    let json_str = &input[start..end];
    let json_value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Invalid JSON: {}", e))?;

    let remaining = &input[end..];
    Ok((json_value, remaining))
}

/// Convert a JSON value to command definitions.
/// Each JSON object becomes one command. Nested objects in "params" are flattened.
fn commands_from_json_value(value: serde_json::Value) -> Vec<CommandDefinition> {
    let mut commands = Vec::new();

    match value {
        serde_json::Value::Object(obj) => {
            // Use the first key's value as command name if the key is a common pattern like "cmd", "command", "action"
            // Otherwise use the first key itself
            let default_key = String::from("command");
            let first_key = obj.keys().next().unwrap_or(&default_key);
            let command_name = if ["cmd", "command", "action", "type", "method", "op"].contains(&first_key.as_str()) {
                // Use the value of this key as the command name
                if let Some(serde_json::Value::String(name_val)) = obj.get(first_key) {
                    // Sanitize the name: lowercase, replace non-alphanumeric with underscore
                    name_val
                        .to_lowercase()
                        .replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
                        .split('_')
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("_")
                } else if let Some(serde_json::Value::Number(n)) = obj.get(first_key) {
                    n.to_string()
                } else {
                    first_key.to_string()
                }
            } else {
                first_key.to_string()
            };

            // Build payload template and parameters
            let mut template_fields = Vec::new();
            let mut parameters = Vec::new();

            for (key, val) in &obj {
                if key == "params" && val.is_object() {
                    // Flatten params object into individual parameters
                    if let serde_json::Value::Object(params_obj) = val {
                        for (param_key, param_val) in params_obj {
                            template_fields
                                .push(format!(r#""{}": ${{{{{}}}}}"#, param_key, param_key));

                            let data_type = infer_data_type(param_val);
                            parameters.push(ParameterDefinition {
                                name: param_key.clone(),
                                display_name: generate_display_name(param_key),
                                data_type,
                                default_value: json_to_metric_value(param_val),
                                min: None,
                                max: None,
                                unit: String::new(),
                                allowed_values: vec![],
                                required: false,
                                visible_when: None,
                                group: None,
                                help_text: String::new(),
                                validation: vec![],
                            });
                        }
                        // Keep params in template as nested object
                        let param_templates: Vec<String> = params_obj
                            .keys()
                            .map(|k| format!(r#""{}": ${{{{{}}}}}"#, k, k))
                            .collect();
                        template_fields
                            .push(format!(r#""params": {{{}}}"#, param_templates.join(", ")));
                    }
                } else {
                    // Regular field
                    template_fields.push(format!(r#""{}": ${{{{{}}}}}"#, key, key));

                    let data_type = infer_data_type(val);
                    parameters.push(ParameterDefinition {
                        name: key.clone(),
                        display_name: generate_display_name(key),
                        data_type,
                        default_value: json_to_metric_value(val),
                        min: None,
                        max: None,
                        unit: String::new(),
                        allowed_values: vec![],
                        required: false,
                        visible_when: None,
                        group: None,
                        help_text: String::new(),
                        validation: vec![],
                    });
                }
            }

            // Build payload template
            let payload_template = format!("{{{}}}", template_fields.join(", "));

            commands.push(CommandDefinition {
                name: command_name.clone(),
                display_name: generate_display_name(&command_name),
                payload_template,
                parameters,
                samples: vec![],
                llm_hints: String::new(),
                fixed_values: std::collections::HashMap::new(),
                parameter_groups: vec![],
            });
        }
        serde_json::Value::Array(arr) => {
            // Process each element in the array
            for item in arr {
                commands.extend(commands_from_json_value(item));
            }
        }
        _ => {
            // Primitive value, create a simple command
            commands.push(CommandDefinition {
                name: "set_value".to_string(),
                display_name: "Set Value".to_string(),
                payload_template: r#"{"value": "${value}"}"#.to_string(),
                parameters: vec![ParameterDefinition {
                    name: "value".to_string(),
                    display_name: "Value".to_string(),
                    data_type: infer_data_type(&value),
                    default_value: None,
                    min: None,
                    max: None,
                    unit: String::new(),
                    allowed_values: vec![],
                    required: false,
                    visible_when: None,
                    group: None,
                    help_text: String::new(),
                    validation: vec![],
                }],
                samples: vec![],
                llm_hints: String::new(),
                fixed_values: std::collections::HashMap::new(),
                parameter_groups: vec![],
            });
        }
    }

    commands
}

/// Infer unit from field name patterns.
fn infer_unit(field_name: &str) -> &'static str {
    let lower = field_name.to_lowercase();

    // Battery/power related
    if lower.contains("battery") || lower.contains("batt") {
        return "%";
    }
    if lower.contains("voltage") || lower.contains("volt") {
        return "V";
    }
    if lower.contains("current") || lower.contains("amp") {
        return "A";
    }
    if lower.contains("power") {
        return "W";
    }
    if lower.contains("energy") {
        return "kWh";
    }

    // Temperature related
    if lower.contains("temp") || lower.contains("temperature") {
        return "°C";
    }

    // Humidity related
    if lower.contains("humidity") || lower.contains("humid") {
        return "%";
    }

    // Pressure related
    if lower.contains("pressure") || lower.contains("press") {
        return "hPa";
    }

    // Light related
    if lower.contains("lux") || lower.contains("light") || lower.contains("illuminance") {
        return "lx";
    }

    // Speed/velocity
    if lower.contains("speed") || lower.contains("velocity") {
        return "m/s";
    }

    // Frequency
    if lower.contains("freq") || lower.contains("frequency") || lower.contains("hz") {
        return "Hz";
    }

    // Distance/position
    if lower.contains("distance") || lower.contains("position") {
        return "m";
    }

    // Weight/mass
    if lower.contains("weight") || lower.contains("mass") {
        return "kg";
    }

    // RSSI/signal strength
    if lower.contains("rssi") || lower.contains("signal") || lower.contains("snr") {
        return "dBm";
    }

    // Percentage
    if lower.contains("level") || lower.contains("pct") || lower.contains("percent") {
        return "%";
    }

    // Timestamp
    if lower.contains("ts") || lower.contains("time") || lower.contains("timestamp") {
        return "";
    }

    "" // No unit inferred
}

/// Generate display name from field name.
fn generate_display_name(field_name: &str) -> String {
    // Split by common separators: ., _, -
    let parts: Vec<&str> = field_name.split(&['.', '_', '-'][..]).collect();

    // Capitalize each part and join with space
    let display: Vec<String> = parts
        .iter()
        .map(|part| {
            if part.is_empty() {
                String::new()
            } else {
                let mut chars = part.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            }
        })
        .filter(|s| !s.is_empty())
        .collect();

    if display.is_empty() {
        field_name.to_string()
    } else {
        display.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_flatten_json_simple() {
        let input = json!({
            "temperature": 25.5,
            "humidity": 60
        });

        let result = flatten_json(&input, "");
        assert_eq!(result.len(), 2);
        assert!(result.contains_key("temperature"));
        assert!(result.contains_key("humidity"));
    }

    #[test]
    fn test_flatten_json_nested_2_levels() {
        let input = json!({
            "temp": {
                "value": 25.5
            }
        });

        let result = flatten_json(&input, "");
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("temp.value"));
        assert_eq!(result["temp.value"], 25.5);
    }

    #[test]
    fn test_flatten_json_nested_5_levels() {
        let input = json!({
            "a": {
                "b": {
                    "c": {
                        "d": {
                            "e": 42
                        }
                    }
                }
            }
        });

        let result = flatten_json(&input, "");
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("a.b.c.d.e"));
        assert_eq!(result["a.b.c.d.e"], 42);
    }

    #[test]
    fn test_flatten_json_nested_10_levels() {
        let input = json!({
            "l1": {
                "l2": {
                    "l3": {
                        "l4": {
                            "l5": {
                                "l6": {
                                    "l7": {
                                        "l8": {
                                            "l9": {
                                                "l10": "deep_value"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let result = flatten_json(&input, "");
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("l1.l2.l3.l4.l5.l6.l7.l8.l9.l10"));
        assert_eq!(result["l1.l2.l3.l4.l5.l6.l7.l8.l9.l10"], "deep_value");
    }

    #[test]
    fn test_flatten_json_mixed_nesting() {
        let input = json!({
            "simple": 1,
            "nested": {
                "value": 2
            },
            "deep": {
                "level1": {
                    "level2": 3
                }
            }
        });

        let result = flatten_json(&input, "");
        assert_eq!(result.len(), 3);
        assert_eq!(result["simple"], 1);
        assert_eq!(result["nested.value"], 2);
        assert_eq!(result["deep.level1.level2"], 3);
    }

    #[test]
    fn test_flatten_json_with_prefix() {
        let input = json!({
            "value": 25.5
        });

        let result = flatten_json(&input, "sensor");
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("sensor.value"));
    }

    #[test]
    fn test_generate_display_name_nested() {
        assert_eq!(generate_display_name("temp.value.new"), "Temp Value New");
        assert_eq!(
            generate_display_name("sensor_data.temperature"),
            "Sensor Data Temperature"
        );
        assert_eq!(generate_display_name("a.b.c.d.e.f"), "A B C D E F");
    }

    #[test]
    fn test_infer_unit_deep_nested_field() {
        // Unit inference should work on any part of the nested path
        assert_eq!(infer_unit("data.sensor.temperature"), "°C");
        assert_eq!(infer_unit("values.humidity"), "%");
        assert_eq!(infer_unit("deep.nested.battery.level"), "%");
    }

    #[test]
    fn test_flatten_json_array_of_objects() {
        let input = json!({
            "readings": [
                {"sensor": "temp1", "value": 20.5},
                {"sensor": "temp2", "value": 22.3}
            ]
        });

        let result = flatten_json(&input, "");
        assert!(result.contains_key("readings.0.sensor"));
        assert!(result.contains_key("readings.0.value"));
        assert!(result.contains_key("readings.1.sensor"));
        assert!(result.contains_key("readings.1.value"));
        assert_eq!(result["readings.0.value"], 20.5);
        assert_eq!(result["readings.1.value"], 22.3);
    }

    #[test]
    fn test_flatten_json_nested_array_of_objects() {
        let input = json!({
            "data": {
                "sensors": [
                    {
                        "id": "temp1",
                        "readings": [
                            {"timestamp": 1000, "value": 20.0},
                            {"timestamp": 2000, "value": 21.0}
                        ]
                    }
                ]
            }
        });

        let result = flatten_json(&input, "");
        assert!(result.contains_key("data.sensors.0.id"));
        assert!(result.contains_key("data.sensors.0.readings.0.timestamp"));
        assert!(result.contains_key("data.sensors.0.readings.0.value"));
        assert!(result.contains_key("data.sensors.0.readings.1.timestamp"));
        assert!(result.contains_key("data.sensors.0.readings.1.value"));
        assert_eq!(result["data.sensors.0.id"], "temp1");
        assert_eq!(result["data.sensors.0.readings.0.value"], 20.0);
        assert_eq!(result["data.sensors.0.readings.1.value"], 21.0);
    }

    #[test]
    fn test_flatten_json_array_of_primitives() {
        let input = json!({
            "values": [10, 20, 30]
        });

        let result = flatten_json(&input, "");
        assert!(result.contains_key("values.0"));
        assert!(result.contains_key("values.1"));
        assert!(result.contains_key("values.2"));
        assert_eq!(result["values.0"], 10);
        assert_eq!(result["values.1"], 20);
        assert_eq!(result["values.2"], 30);
    }

    #[test]
    fn test_flatten_json_complex_iot_structure() {
        // Real-world IoT data with nested structure
        let input = json!({
            "device": {
                "id": "sensor-001",
                "metadata": {
                    "location": "building1.floor2.room3",
                    "type": "temperature"
                }
            },
            "measurements": [
                {
                    "timestamp": 1234567890,
                    "data": {
                        "temperature": {
                            "value": 23.5,
                            "unit": "celsius"
                        },
                        "humidity": {
                            "value": 65,
                            "unit": "percent"
                        }
                    }
                }
            ]
        });

        let result = flatten_json(&input, "");

        // Check deeply nested paths
        assert!(result.contains_key("device.id"));
        assert!(result.contains_key("device.metadata.location"));
        assert!(result.contains_key("device.metadata.type"));
        assert!(result.contains_key("measurements.0.timestamp"));
        assert!(result.contains_key("measurements.0.data.temperature.value"));
        assert!(result.contains_key("measurements.0.data.temperature.unit"));
        assert!(result.contains_key("measurements.0.data.humidity.value"));

        assert_eq!(result["device.id"], "sensor-001");
        assert_eq!(result["device.metadata.location"], "building1.floor2.room3");
        assert_eq!(result["measurements.0.data.temperature.value"], 23.5);
        assert_eq!(result["measurements.0.data.humidity.value"], 65);
    }

    #[test]
    fn test_parse_one_json_single_object() {
        let input = r#"{"action": "turn_on"}"#;
        let (json, rest) = parse_one_json(input).unwrap();
        assert_eq!(json["action"], "turn_on");
        assert_eq!(rest, "");
    }

    #[test]
    fn test_parse_one_json_with_whitespace() {
        let input = r#"  {"action": "turn_on"}  "#;
        let (json, rest) = parse_one_json(input).unwrap();
        assert_eq!(json["action"], "turn_on");
        assert_eq!(rest.trim(), "");
    }

    #[test]
    fn test_parse_one_json_multiple_objects() {
        let input = r#"{"action": "on"} {"action": "off"}"#;
        let (json1, rest1) = parse_one_json(input).unwrap();
        assert_eq!(json1["action"], "on");

        let (json2, rest2) = parse_one_json(rest1).unwrap();
        assert_eq!(json2["action"], "off");
        assert_eq!(rest2, "");
    }

    #[test]
    fn test_parse_one_json_nested_object() {
        let input = r#"{"action": "set_config", "params": {"brightness": 50, "color": "red"}}"#;
        let (json, rest) = parse_one_json(input).unwrap();
        assert_eq!(json["action"], "set_config");
        assert_eq!(json["params"]["brightness"], 50);
        assert_eq!(json["params"]["color"], "red");
        assert_eq!(rest, "");
    }

    #[test]
    fn test_parse_one_json_with_string_containing_braces() {
        let input = r#"{"action": "send", "message": "Hello {world}"}"#;
        let (json, rest) = parse_one_json(input).unwrap();
        assert_eq!(json["action"], "send");
        assert_eq!(json["message"], "Hello {world}");
        assert_eq!(rest, "");
    }

    #[test]
    fn test_commands_from_json_value_single_object() {
        let json = json!({"action": "turn_on", "brightness": 100});
        let commands = commands_from_json_value(json);

        assert_eq!(commands.len(), 1);
        // Command name is now the value of "action" key
        assert_eq!(commands[0].name, "turn_on");
        assert_eq!(commands[0].display_name, "Turn On");
        // Template uses double braces: ${{action}}
        assert!(commands[0].payload_template.contains("${{action}}"));
        assert!(commands[0].payload_template.contains("${{brightness}}"));
        assert_eq!(commands[0].parameters.len(), 2);
        assert_eq!(commands[0].parameters[0].name, "action");
        assert_eq!(commands[0].parameters[1].name, "brightness");
    }

    #[test]
    fn test_commands_from_json_value_array() {
        let json = json!( [
            {"action": "turn_on"},
            {"action": "turn_off"},
            {"action": "set_brightness", "value": 50}
        ]);
        let commands = commands_from_json_value(json);

        assert_eq!(commands.len(), 3);
        // Command names are now the values of "action" key
        assert_eq!(commands[0].name, "turn_on");
        assert_eq!(commands[1].name, "turn_off");
        assert_eq!(commands[2].name, "set_brightness");
    }

    #[test]
    fn test_parse_downlink_commands_single_json() {
        let input = r#"{"action": "turn_on", "brightness": 100}"#;
        let commands = parse_downlink_commands(input).unwrap();

        assert_eq!(commands.len(), 1);
        // Command name is now the value of "action" key
        assert_eq!(commands[0].name, "turn_on");
    }

    #[test]
    fn test_parse_downlink_commands_multiple_json() {
        let input = r#"{"action": "turn_on"} {"action": "turn_off"} {"action": "set_brightness", "value": 80}"#;
        let commands = parse_downlink_commands(input).unwrap();

        assert_eq!(commands.len(), 3);
        // Command names are now the values of "action" key
        assert_eq!(commands[0].name, "turn_on");
        assert_eq!(commands[0].display_name, "Turn On");
        assert_eq!(commands[1].name, "turn_off");
        assert_eq!(commands[2].name, "set_brightness");
        assert_eq!(commands[2].parameters.len(), 2); // action and value
    }

    #[test]
    fn test_parse_downlink_commands_with_newlines() {
        let input = r#"
            {"action": "turn_on"}
            {"action": "turn_off"}
        "#;
        let commands = parse_downlink_commands(input).unwrap();

        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn test_parse_downlink_commands_complex_nested() {
        let input = r#"
            {"action": "set_config", "params": {"brightness": 50}}
            {"action": "send", "data": {"message": "hello"}}
        "#;
        let commands = parse_downlink_commands(input).unwrap();

        assert_eq!(commands.len(), 2);
        // Command names are now the values of "action" key
        assert_eq!(commands[0].name, "set_config");
        assert_eq!(commands[0].parameters.len(), 2); // action and params
        assert_eq!(commands[1].name, "send");
    }

    #[test]
    fn test_commands_from_json_value_cmd_pattern() {
        // Device protocol pattern: {"cmd": "capture", "request_id": "...", "params": {...}}
        let json = json!({
            "cmd": "capture",
            "request_id": "req-001",
            "params": {
                "enable_ai": true,
                "chunk_size": 0,
                "store_to_sd": false
            }
        });
        let commands = commands_from_json_value(json);

        assert_eq!(commands.len(), 1);
        // Command name is now the value of "cmd" key
        assert_eq!(commands[0].name, "capture");
        assert_eq!(commands[0].display_name, "Capture");

        // Payload template should have all fields
        assert!(commands[0].payload_template.contains("${{cmd}}"));
        assert!(commands[0].payload_template.contains(r#""params": {"#));
        assert!(commands[0].payload_template.contains("${{enable_ai}}"));
        assert!(commands[0].payload_template.contains("${{chunk_size}}"));
        assert!(commands[0].payload_template.contains("${{store_to_sd}}"));
        assert!(commands[0].payload_template.contains("${{request_id}}"));

        // Parameters should include cmd, flattened params fields, request_id (order may vary)
        assert_eq!(commands[0].parameters.len(), 5);
        let param_names: Vec<&str> = commands[0].parameters.iter().map(|p| p.name.as_str()).collect();
        assert!(param_names.contains(&"cmd"));
        assert!(param_names.contains(&"request_id"));
        assert!(param_names.contains(&"enable_ai"));
        assert!(param_names.contains(&"chunk_size"));
        assert!(param_names.contains(&"store_to_sd"));
    }

    #[test]
    fn test_commands_from_json_value_sleep_cmd() {
        let json = json!({
            "cmd": "sleep",
            "request_id": "req-002",
            "params": {
                "duration_sec": 60
            }
        });
        let commands = commands_from_json_value(json);

        assert_eq!(commands.len(), 1);
        // Command name is now the value of "cmd" key
        assert_eq!(commands[0].name, "sleep");

        // Check parameters: cmd, request_id, duration_sec (order may vary)
        assert_eq!(commands[0].parameters.len(), 3);
        let param_names: Vec<&str> = commands[0].parameters.iter().map(|p| p.name.as_str()).collect();
        assert!(param_names.contains(&"cmd"));
        assert!(param_names.contains(&"request_id"));
        assert!(param_names.contains(&"duration_sec"));

        // Find cmd parameter and check its default value
        let cmd_param = commands[0].parameters.iter().find(|p| p.name == "cmd").unwrap();
        if let Some(MetricValue::String(val)) = &cmd_param.default_value {
            assert_eq!(val, "sleep");
        } else {
            panic!("Expected String default value for cmd parameter");
        }
    }

    #[test]
    fn test_commands_from_json_value_multiple_cmd_pattern() {
        // Multiple commands with cmd/params pattern
        let input = r#"
            {"cmd": "capture", "request_id": "req-001", "params": {"enable_ai": true}}
            {"cmd": "sleep", "request_id": "req-002", "params": {"duration_sec": 60}}
        "#;
        let commands = parse_downlink_commands(input).unwrap();

        assert_eq!(commands.len(), 2);

        // Command names are now the values of "cmd" key - unique!
        assert_eq!(commands[0].name, "capture");
        assert_eq!(commands[1].name, "sleep");
    }

    #[test]
    fn test_commands_from_json_value_simple_action_pattern() {
        // Simple pattern: {"action": "turn_on", "brightness": 100}
        let json = json!({"action": "turn_on", "brightness": 100});
        let commands = commands_from_json_value(json);

        assert_eq!(commands.len(), 1);
        // Command name is now the value of "action" key
        assert_eq!(commands[0].name, "turn_on");
        assert_eq!(commands[0].parameters.len(), 2);
    }

    #[test]
    fn test_commands_with_command_field() {
        // Using "command" as the first key
        let json = json!({
            "command": "restart",
            "request_id": "req-003"
        });
        let commands = commands_from_json_value(json);

        assert_eq!(commands.len(), 1);
        // Command name is now the value of "command" key
        assert_eq!(commands[0].name, "restart");
    }

    #[test]
    fn test_commands_with_nested_params() {
        // Test that params object is properly flattened
        let json = json!({
            "action": "configure",
            "params": {
                "brightness": 80,
                "color": "red"
            }
        });
        let commands = commands_from_json_value(json);

        assert_eq!(commands.len(), 1);
        // Command name is now the value of "action" key
        assert_eq!(commands[0].name, "configure");

        // Parameters: action, brightness, color (params is flattened, order may vary)
        assert_eq!(commands[0].parameters.len(), 3);
        let param_names: Vec<&str> = commands[0].parameters.iter().map(|p| p.name.as_str()).collect();
        assert!(param_names.contains(&"action"));
        assert!(param_names.contains(&"brightness"));
        assert!(param_names.contains(&"color"));

        // Payload template should have nested params object
        assert!(commands[0].payload_template.contains(r#""params": {"#));
        assert!(commands[0].payload_template.contains("${{brightness}}"));
        assert!(commands[0].payload_template.contains("${{color}}"));
    }
}
