//! MDL (Message Definition Language) generation handlers.

use axum::{extract::State, Json};
use serde_json::json;
use std::collections::{HashMap, BTreeMap};

use edge_ai_devices::{
    mdl::MetricDataType,
    mdl_format::{DeviceTypeDefinition, UplinkConfig, DownlinkConfig, MetricDefinition, CommandDefinition},
};

use crate::handlers::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;
use super::models::GenerateMdlRequest;

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
    let device_type = req.device_name
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
                    flattened.into_iter().map(|(name, value)| {
                        MetricDefinition {
                            name: name.clone(),
                            display_name: generate_display_name(&name),
                            data_type: infer_data_type(&value),
                            unit: infer_unit(&name).to_string(),
                            min: None,
                            max: None,
                            required: false,
                        }
                    }).collect()
                }
            }
            Err(e) => {
                return Err(ErrorResponse::bad_request(format!("Invalid uplink example JSON: {}", e)));
            }
        }
    };

    // Parse and flatten downlink example
    let downlink_commands = if req.downlink_example.trim().is_empty() {
        vec![]
    } else {
        match serde_json::from_str::<serde_json::Value>(&req.downlink_example) {
            Ok(sample) => {
                let flattened = flatten_json(&sample, "");

                if flattened.is_empty() {
                    vec![]
                } else {
                    // Create commands from downlink fields
                    // For simplicity, we create one command per field
                    flattened.into_iter().map(|(name, _)| {
                        CommandDefinition {
                            name: name.clone(),
                            display_name: generate_display_name(&name),
                            payload_template: format!(r#"{{"{}": "${{value}}"}}"#, name),
                            parameters: vec![],
                        }
                    }).collect()
                }
            }
            Err(e) => {
                return Err(ErrorResponse::bad_request(format!("Invalid downlink example JSON: {}", e)));
            }
        }
    };

    // Build the complete MDL definition
    let def = DeviceTypeDefinition {
        device_type: device_type.clone(),
        name: req.device_name.clone(),
        description: req.description.clone(),
        categories: vec!["sensor".to_string()],
        uplink: UplinkConfig {
            metrics: uplink_metrics,
        },
        downlink: DownlinkConfig {
            commands: downlink_commands,
        },
    };

    ok(def)
}

/// Flatten a JSON value into a map of dot-separated keys to values.
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
                // Recursively flatten nested objects
                if val.is_object() {
                    result.extend(flatten_json(val, &new_key));
                } else {
                    result.insert(new_key, val.clone());
                }
            }
        }
        serde_json::Value::Array(_) => {
            // For arrays, we just store the prefix with the array value
            // Not expanding arrays for now as they're less common in IoT
            if !prefix.is_empty() {
                result.insert(prefix.to_string(), value.clone());
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
            if n.is_i64() {
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

    ""  // No unit inferred
}

/// Generate display name from field name.
fn generate_display_name(field_name: &str) -> String {
    // Split by common separators: ., _, -
    let parts: Vec<&str> = field_name
        .split(&['.', '_', '-'][..])
        .collect();

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
                    Some(first) => {
                        first.to_uppercase().collect::<String>() + chars.as_str()
                    }
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
