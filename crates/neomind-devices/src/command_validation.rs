//! Shared command validation with schema feedback for device commands.
//!
//! One preflight layer consumed by every entry point (HTTP handler, CLI via
//! HTTP, chat agent via shell). Wrong command → the available command names;
//! wrong params → the offending command's schema (types, required,
//! allowed_values, min/max, default, unit) so the caller — human or model —
//! can correct in one round-trip. Feedback is bounded: the schema of the
//! requested command only, never the whole catalog on a param error.
//!
//! Ported from the retired chat `device_command` tool; the DeviceService
//! validation remains the final authority — this layer adds the corrective
//! feedback the service error strings never carried.

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::mdl::MetricDataType;
use crate::mdl_format::ParameterDefinition;
use crate::registry::DeviceTypeTemplate;

/// One command from a device type template, in caller-facing form.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub parameters: Vec<ParamSpec>,
}

/// One parameter of a command, with the template's constraints attached.
#[derive(Debug, Clone)]
pub struct ParamSpec {
    pub name: String,
    pub display_name: String,
    pub data_type: String,
    pub required: bool,
    pub description: String,
    /// Allowed values from the template (`allowed_values` and/or Enum options).
    pub allowed_values: Option<Vec<Value>>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub default: Option<Value>,
    pub unit: Option<String>,
}

/// What a failed preflight tells the caller.
///
/// `message` states the reason; `hint` (when present) is compact JSON — the
/// schema of the offending command, or the command-name list — ready for the
/// API `hint` field the CLI appends to error output.
#[derive(Debug, Clone)]
pub struct PreflightError {
    pub message: String,
    pub hint: Option<String>,
}

impl PreflightError {
    fn with_schema(message: String, schema: &Value) -> Self {
        Self {
            message,
            hint: Some(serde_json::to_string(schema).unwrap_or_default()),
        }
    }
}

/// Build the caller-facing command catalog from a device type template.
///
/// No device-type hardcoding: whatever the template declares is what the
/// caller sees. Enum options merge into `allowed_values` when both exist.
pub fn catalog_from_template(template: &DeviceTypeTemplate) -> Vec<CommandSpec> {
    template
        .commands
        .iter()
        .map(|cmd| CommandSpec {
            name: cmd.name.clone(),
            display_name: cmd.display_name.clone(),
            description: cmd.description.clone(),
            parameters: cmd.parameters.iter().map(param_from_template).collect(),
        })
        .collect()
}

fn param_from_template(p: &ParameterDefinition) -> ParamSpec {
    let (data_type, enum_options) = match &p.data_type {
        MetricDataType::Integer => ("integer".to_string(), None),
        MetricDataType::Float => ("float".to_string(), None),
        MetricDataType::String => ("string".to_string(), None),
        MetricDataType::Boolean => ("boolean".to_string(), None),
        MetricDataType::Binary => ("binary".to_string(), None),
        MetricDataType::Enum { options } => ("enum".to_string(), Some(options.clone())),
        MetricDataType::Array { .. } => ("array".to_string(), None),
    };
    let mut allowed: Vec<Value> = p.allowed_values.iter().map(|v| v.to_json_value()).collect();
    if let Some(opts) = enum_options {
        for o in opts {
            let v = Value::String(o);
            if !allowed.contains(&v) {
                allowed.push(v);
            }
        }
    }
    ParamSpec {
        name: p.name.clone(),
        display_name: p.display_name.clone(),
        data_type,
        required: p.required,
        description: if p.help_text.is_empty() {
            String::new()
        } else {
            p.help_text.clone()
        },
        allowed_values: if allowed.is_empty() {
            None
        } else {
            Some(allowed)
        },
        min: p.min,
        max: p.max,
        default: p.default_value.as_ref().map(|v| v.to_json_value()),
        unit: if p.unit.is_empty() {
            None
        } else {
            Some(p.unit.clone())
        },
    }
}

/// Schema JSON for one command — the corrective payload on param errors.
pub fn command_schema_json(cmd: &CommandSpec) -> Value {
    json!({
        "command": cmd.name,
        "display_name": cmd.display_name,
        "description": cmd.description,
        "parameters": cmd.parameters.iter().map(param_schema_json).collect::<Vec<_>>(),
    })
}

fn param_schema_json(p: &ParamSpec) -> Value {
    let mut obj = json!({
        "name": p.name,
        "display_name": p.display_name,
        "type": p.data_type,
        "required": p.required,
        "description": p.description,
    });
    if let Some(map) = obj.as_object_mut() {
        if let Some(vals) = &p.allowed_values {
            map.insert("allowed_values".into(), Value::Array(vals.clone()));
        }
        if let Some(min) = p.min {
            map.insert("min".into(), json!(min));
        }
        if let Some(max) = p.max {
            map.insert("max".into(), json!(max));
        }
        if let Some(default) = &p.default {
            map.insert("default".into(), default.clone());
        }
        if let Some(unit) = &p.unit {
            if !unit.is_empty() {
                map.insert("unit".into(), json!(unit));
            }
        }
    }
    obj
}

/// Preflight a caller's command + params against the template catalog.
///
/// Checks the things the caller can correct immediately: unknown command
/// name, missing required params, unknown keys, type mismatches, enum and
/// range violations. Template constraints only — no extra rules invented
/// here; `DeviceService::validate_command_params` remains the authority.
pub fn preflight(
    commands: &[CommandSpec],
    command: &str,
    params: &HashMap<String, Value>,
) -> Result<(), PreflightError> {
    let Some(spec) = commands.iter().find(|c| c.name == command) else {
        let available: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        return Err(PreflightError {
            message: format!(
                "Command '{}' is not in this device type's template. Available commands: {}.",
                command,
                if available.is_empty() {
                    "(none)".to_string()
                } else {
                    available.join(", ")
                }
            ),
            hint: Some(
                serde_json::to_string(&json!({ "available_commands": available }))
                    .unwrap_or_default(),
            ),
        });
    };

    let missing: Vec<&str> = spec
        .parameters
        .iter()
        .filter(|p| p.required && !params.contains_key(&p.name) && p.default.is_none())
        .map(|p| p.name.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(PreflightError::with_schema(
            format!(
                "Command '{}' is missing required params: {}.",
                command,
                missing.join(", ")
            ),
            &command_schema_json(spec),
        ));
    }

    let known: std::collections::HashSet<&str> =
        spec.parameters.iter().map(|p| p.name.as_str()).collect();
    let unknown: Vec<&str> = params
        .keys()
        .filter(|k| !known.contains(k.as_str()))
        .map(|k| k.as_str())
        .collect();
    if !unknown.is_empty() {
        return Err(PreflightError::with_schema(
            format!(
                "Unknown param(s) for command '{}': {}. Allowed: {}.",
                command,
                unknown.join(", "),
                if known.is_empty() {
                    "(none)".to_string()
                } else {
                    known.into_iter().collect::<Vec<_>>().join(", ")
                }
            ),
            &command_schema_json(spec),
        ));
    }

    for p in &spec.parameters {
        let Some(raw) = params.get(&p.name) else {
            continue;
        };
        if raw.is_null() {
            continue;
        }
        if let Some(msg) = type_mismatch_message(p, raw) {
            return Err(PreflightError::with_schema(
                format!("Command '{}': {msg}", command),
                &command_schema_json(spec),
            ));
        }
        if let Some(allowed) = &p.allowed_values {
            if !allowed.is_empty() && !allowed.iter().any(|a| json_values_loosely_equal(a, raw)) {
                return Err(PreflightError::with_schema(
                    format!(
                        "Command '{}': param '{}' value {} is not in allowed_values {:?}.",
                        command, p.name, raw, allowed
                    ),
                    &command_schema_json(spec),
                ));
            }
        }
        if p.data_type == "integer" || p.data_type == "float" {
            let num = raw.as_f64().or_else(|| raw.as_i64().map(|i| i as f64));
            if let Some(n) = num {
                if let Some(min) = p.min {
                    if n < min {
                        return Err(PreflightError::with_schema(
                            format!(
                                "Command '{}': param '{}' value {} is below minimum {}{}.",
                                command,
                                p.name,
                                n,
                                min,
                                p.unit.as_deref().unwrap_or("")
                            ),
                            &command_schema_json(spec),
                        ));
                    }
                }
                if let Some(max) = p.max {
                    if n > max {
                        return Err(PreflightError::with_schema(
                            format!(
                                "Command '{}': param '{}' value {} is above maximum {}{}.",
                                command,
                                p.name,
                                n,
                                max,
                                p.unit.as_deref().unwrap_or("")
                            ),
                            &command_schema_json(spec),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn type_mismatch_message(p: &ParamSpec, raw: &Value) -> Option<String> {
    let ok = match p.data_type.as_str() {
        "integer" => {
            raw.is_i64() || raw.is_u64() || raw.as_str().is_some_and(|s| s.parse::<i64>().is_ok())
        }
        "float" => {
            raw.is_f64()
                || raw.is_i64()
                || raw.is_u64()
                || raw.as_str().is_some_and(|s| s.parse::<f64>().is_ok())
        }
        "boolean" => {
            raw.is_boolean()
                || raw.as_str().is_some_and(|s| {
                    matches!(
                        s.to_lowercase().as_str(),
                        "true" | "false" | "1" | "0" | "yes" | "no" | "on" | "off"
                    )
                })
        }
        "string" | "enum" | "binary" => raw.is_string() || raw.is_number() || raw.is_boolean(),
        "array" => raw.is_array(),
        _ => true, // unknown type label — don't invent a hard fail
    };
    if ok {
        None
    } else {
        Some(format!(
            "param '{}' expects type '{}', got {}.",
            p.name, p.data_type, raw
        ))
    }
}

fn json_values_loosely_equal(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (Value::String(s), Value::Number(n)) | (Value::Number(n), Value::String(s)) => {
            if let Some(i) = n.as_i64() {
                return s.parse::<i64>().ok() == Some(i);
            }
            if let Some(f) = n.as_f64() {
                return s
                    .parse::<f64>()
                    .ok()
                    .is_some_and(|x| (x - f).abs() < f64::EPSILON);
            }
            false
        }
        (Value::Number(n1), Value::Number(n2)) => match (n1.as_f64(), n2.as_f64()) {
            (Some(a), Some(b)) => (a - b).abs() < f64::EPSILON,
            _ => false,
        },
        (Value::Bool(b), Value::String(s)) | (Value::String(s), Value::Bool(b)) => {
            matches!(
                (s.to_lowercase().as_str(), *b),
                ("true" | "1" | "yes" | "on", true) | ("false" | "0" | "no" | "off", false)
            )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec_catalog() -> Vec<CommandSpec> {
        vec![
            CommandSpec {
                name: "reboot".into(),
                display_name: "Reboot".into(),
                description: "Restart device".into(),
                parameters: vec![],
            },
            CommandSpec {
                name: "render_text".into(),
                display_name: "Render Text".into(),
                description: "Show text".into(),
                parameters: vec![ParamSpec {
                    name: "text".into(),
                    display_name: "Text".into(),
                    data_type: "string".into(),
                    required: true,
                    description: "Content".into(),
                    allowed_values: None,
                    min: None,
                    max: None,
                    default: None,
                    unit: None,
                }],
            },
            CommandSpec {
                name: "set_brightness".into(),
                display_name: "Set Brightness".into(),
                description: "Brightness".into(),
                parameters: vec![ParamSpec {
                    name: "level".into(),
                    display_name: "Level".into(),
                    data_type: "integer".into(),
                    required: true,
                    description: "Level".into(),
                    allowed_values: None,
                    min: Some(0.0),
                    max: Some(100.0),
                    default: Some(json!(50)),
                    unit: Some("%".into()),
                }],
            },
            CommandSpec {
                name: "set_mode".into(),
                display_name: "Set Mode".into(),
                description: "Mode".into(),
                parameters: vec![ParamSpec {
                    name: "mode".into(),
                    display_name: "Mode".into(),
                    data_type: "enum".into(),
                    required: true,
                    description: "Mode".into(),
                    allowed_values: Some(vec![json!("auto"), json!("manual")]),
                    min: None,
                    max: None,
                    default: Some(json!("auto")),
                    unit: None,
                }],
            },
        ]
    }

    fn params(v: Value) -> HashMap<String, Value> {
        match v {
            Value::Object(map) => map.into_iter().collect(),
            _ => panic!("object expected"),
        }
    }

    #[test]
    fn unknown_command_lists_available() {
        let err = preflight(&spec_catalog(), "nope", &HashMap::new()).unwrap_err();
        assert!(err.message.contains("Available commands"));
        assert!(err.message.contains("render_text"));
        assert!(err
            .hint
            .as_deref()
            .unwrap_or("")
            .contains("available_commands"));
    }

    #[test]
    fn missing_required_returns_schema() {
        let err = preflight(&spec_catalog(), "render_text", &params(json!({}))).unwrap_err();
        assert!(err.message.contains("missing required params"));
        let hint: Value = serde_json::from_str(err.hint.as_deref().unwrap()).expect("hint is json");
        assert_eq!(hint["command"], "render_text");
        assert!(hint["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "text" && p["required"] == true));
    }

    #[test]
    fn unknown_param_names_surface_allowed() {
        let err = preflight(
            &spec_catalog(),
            "render_text",
            &params(json!({"text": "hi", "extra": 1})),
        )
        .unwrap_err();
        assert!(err.message.contains("Unknown param"));
        assert!(err.message.contains("extra"));
    }

    #[test]
    fn type_enum_range_feedback() {
        let err = preflight(
            &spec_catalog(),
            "set_brightness",
            &params(json!({"level": {"nested": true}})),
        )
        .unwrap_err();
        assert!(err.message.contains("expects type 'integer'"));
        let hint: Value = serde_json::from_str(err.hint.as_deref().unwrap()).unwrap();
        assert_eq!(hint["parameters"][0]["min"], 0.0);
        assert_eq!(hint["parameters"][0]["max"], 100.0);
        assert_eq!(hint["parameters"][0]["unit"], "%");

        let err = preflight(
            &spec_catalog(),
            "set_mode",
            &params(json!({"mode": "turbo"})),
        )
        .unwrap_err();
        assert!(err.message.contains("allowed_values"));

        let err = preflight(
            &spec_catalog(),
            "set_brightness",
            &params(json!({"level": 150})),
        )
        .unwrap_err();
        assert!(err.message.contains("above maximum"));
    }

    #[test]
    fn valid_calls_pass() {
        assert!(preflight(&spec_catalog(), "reboot", &HashMap::new()).is_ok());
        assert!(preflight(
            &spec_catalog(),
            "render_text",
            &params(json!({"text": "hello"}))
        )
        .is_ok());
        // Coercible string numbers pass; boolean words pass.
        assert!(preflight(
            &spec_catalog(),
            "set_brightness",
            &params(json!({"level": "80"}))
        )
        .is_ok());
    }
}
