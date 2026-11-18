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
use crate::mdl_format::{CommandDefinition, ParameterDefinition};
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
        .map(command_spec_from_definition)
        .collect()
}

/// Build a [`CommandSpec`] from one template command definition.
pub fn command_spec_from_definition(cmd: &CommandDefinition) -> CommandSpec {
    CommandSpec {
        name: cmd.name.clone(),
        display_name: cmd.display_name.clone(),
        description: cmd.description.clone(),
        parameters: cmd.parameters.iter().map(param_from_template).collect(),
    }
}

/// CLI / agent discovery projection for one command.
///
/// Keeps name, description, param defs, and a `--params`-ready
/// `params_example`. Omits protocol envelopes (`samples`,
/// `payload_template`, `fixed_values`) — those stay on the raw API /
/// protocol views. If an example cannot be validated against the same
/// preflight used at invoke time, `params_example` is omitted (defs only).
pub fn invoke_projection(cmd: &CommandSpec) -> Value {
    let mut out = json!({
        "name": cmd.name,
        "description": cmd.description,
        "parameters": cmd.parameters.iter().map(param_schema_json).collect::<Vec<_>>(),
    });
    if let Some(example) = validated_params_example(cmd) {
        if let Some(map) = out.as_object_mut() {
            map.insert("params_example".into(), example);
        }
    }
    out
}

/// Generate a `--params` example object from param defs and validate it.
///
/// Returns `None` when generation is unreliable (preflight rejects the
/// candidate) so callers can show defs only.
pub fn validated_params_example(cmd: &CommandSpec) -> Option<Value> {
    let example = params_example_from_spec(cmd);
    let map = match &example {
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<HashMap<_, _>>(),
        _ => return None,
    };
    // Self-check against the same preflight invoke uses.
    preflight(std::slice::from_ref(cmd), &cmd.name, &map)
        .ok()
        .map(|_| example)
}

/// Best-effort example object for `--params` (unvalidated).
fn params_example_from_spec(cmd: &CommandSpec) -> Value {
    let mut obj = serde_json::Map::new();
    for p in &cmd.parameters {
        // Required always; optional only when a concrete default exists so
        // the example stays minimal and still validates.
        if !p.required && p.default.is_none() {
            continue;
        }
        let sample = if let Some(default) = &p.default {
            default.clone()
        } else if let Some(vals) = &p.allowed_values {
            vals.first().cloned().unwrap_or_else(|| sample_for_type(p))
        } else {
            sample_for_type(p)
        };
        obj.insert(p.name.clone(), sample);
    }
    Value::Object(obj)
}

fn sample_for_type(p: &ParamSpec) -> Value {
    match p.data_type.as_str() {
        "integer" => json!(p.min.map(|m| m as i64).unwrap_or(0)),
        "float" => json!(p.min.unwrap_or(0.0)),
        "boolean" => json!(false),
        "array" => json!([]),
        "binary" => json!(""),
        // Prefer a readable non-empty string for text-like params so models
        // copy a usable `--params` object; other strings stay empty.
        "string" | "enum" => {
            if p.name.eq_ignore_ascii_case("text")
                || p.name.eq_ignore_ascii_case("message")
                || p.name.eq_ignore_ascii_case("content")
            {
                json!("Hello")
            } else {
                json!("")
            }
        }
        _ => json!(""),
    }
}

/// True when the caller's object looks like a device protocol envelope
/// (`{"cmd":...,"params":{...}}`) rather than the command's own params —
/// but only when those keys are NOT declared parameters of the command.
/// Declared `cmd` / `params` param names remain valid inputs.
fn looks_like_protocol_envelope(
    params: &HashMap<String, Value>,
    known: &std::collections::HashSet<&str>,
) -> bool {
    let undeclared_cmd = params.contains_key("cmd") && !known.contains("cmd");
    let undeclared_params_obj =
        params.get("params").is_some_and(|v| v.is_object()) && !known.contains("params");
    undeclared_cmd || undeclared_params_obj
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
///
/// Uses the unvalidated generator for `params_example` so error-path schema
/// building cannot recurse through [`validated_params_example`]'s preflight.
pub fn command_schema_json(cmd: &CommandSpec) -> Value {
    json!({
        "command": cmd.name,
        "display_name": cmd.display_name,
        "description": cmd.description,
        "parameters": cmd.parameters.iter().map(param_schema_json).collect::<Vec<_>>(),
        "input_contract": "--params takes this command's param object (host wraps the protocol envelope)",
        "params_example": params_example_from_spec(cmd),
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
                "Command '{}' was NOT sent: it is not in this device type's template. Available commands: {}.",
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

    let known: std::collections::HashSet<&str> =
        spec.parameters.iter().map(|p| p.name.as_str()).collect();
    // Unvalidated generator only — never call validated_params_example here
    // (that self-checks via preflight and would recurse).
    let example =
        serde_json::to_string(&params_example_from_spec(spec)).unwrap_or_else(|_| "{}".into());
    let envelope = looks_like_protocol_envelope(params, &known);

    let missing: Vec<&str> = spec
        .parameters
        .iter()
        .filter(|p| p.required && !params.contains_key(&p.name) && p.default.is_none())
        .map(|p| p.name.as_str())
        .collect();
    if !missing.is_empty() {
        let mut message = format!(
            "Command '{}' was NOT sent: missing required top-level params: {}. \
             --params takes the command param object (host wraps the protocol). Example: {}.",
            command,
            missing.join(", "),
            example
        );
        if envelope {
            message.push_str(
                " Input looks like a protocol envelope {\"cmd\":...,\"params\":{...}} — \
                 pass the inner command params at the top level instead. Do not auto-unwrap.",
            );
        }
        return Err(PreflightError::with_schema(
            message,
            &command_schema_json(spec),
        ));
    }

    let unknown: Vec<&str> = params
        .keys()
        .filter(|k| !known.contains(k.as_str()))
        .map(|k| k.as_str())
        .collect();
    if !unknown.is_empty() {
        let mut message = format!(
            "Command '{}' was NOT sent: unknown param(s): {}. Allowed: {}. \
             --params takes the command param object. Example: {}.",
            command,
            unknown.join(", "),
            if known.is_empty() {
                "(none)".to_string()
            } else {
                let mut names: Vec<&str> = known.iter().copied().collect();
                names.sort_unstable();
                names.join(", ")
            },
            example
        );
        if envelope {
            message.push_str(
                " Input looks like a protocol envelope {\"cmd\":...,\"params\":{...}} — \
                 pass the inner command params at the top level instead. Do not auto-unwrap.",
            );
        }
        return Err(PreflightError::with_schema(
            message,
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
                format!("Command '{}' was NOT sent: {msg}", command),
                &command_schema_json(spec),
            ));
        }
        if let Some(allowed) = &p.allowed_values {
            if !allowed.is_empty() && !allowed.iter().any(|a| json_values_loosely_equal(a, raw)) {
                return Err(PreflightError::with_schema(
                    format!(
                        "Command '{}' was NOT sent: param '{}' value {} is not in allowed_values {:?}.",
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
                                "Command '{}' was NOT sent: param '{}' value {} is below minimum {}{}.",
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
                                "Command '{}' was NOT sent: param '{}' value {} is above maximum {}{}.",
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
        assert!(err.message.contains("NOT sent"));
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
        assert!(err.message.contains("NOT sent"));
        assert!(err.message.contains("missing required top-level params"));
        assert!(err.message.contains(r#"{"text":"Hello"}"#) || err.message.contains("Hello"));
        let hint: Value = serde_json::from_str(err.hint.as_deref().unwrap()).expect("hint is json");
        assert_eq!(hint["command"], "render_text");
        assert_eq!(hint["params_example"]["text"], "Hello");
        assert!(hint["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "text" && p["required"] == true));
    }

    #[test]
    fn protocol_envelope_is_corrected_not_auto_unwrapped() {
        let err = preflight(
            &spec_catalog(),
            "render_text",
            &params(json!({"cmd":"render_text","params":{"text":"hi"}})),
        )
        .unwrap_err();
        assert!(err.message.contains("NOT sent"));
        assert!(err.message.contains("protocol envelope"));
        assert!(err.message.contains("Do not auto-unwrap"));
        // Must not invent a successful unwrap — still a hard fail.
        assert!(preflight(
            &spec_catalog(),
            "render_text",
            &params(json!({"text": "hi"}))
        )
        .is_ok());
    }

    #[test]
    fn legal_cmd_params_param_names_still_work() {
        let catalog = vec![CommandSpec {
            name: "raw_frame".into(),
            display_name: "Raw".into(),
            description: "Pass-through".into(),
            parameters: vec![
                ParamSpec {
                    name: "cmd".into(),
                    display_name: "Cmd".into(),
                    data_type: "string".into(),
                    required: true,
                    description: String::new(),
                    allowed_values: None,
                    min: None,
                    max: None,
                    default: None,
                    unit: None,
                },
                ParamSpec {
                    name: "params".into(),
                    display_name: "Params".into(),
                    data_type: "string".into(),
                    required: true,
                    description: String::new(),
                    allowed_values: None,
                    min: None,
                    max: None,
                    default: None,
                    unit: None,
                },
            ],
        }];
        assert!(preflight(
            &catalog,
            "raw_frame",
            &params(json!({"cmd": "x", "params": "{}"}))
        )
        .is_ok());
        let proj = invoke_projection(&catalog[0]);
        assert_eq!(proj["params_example"]["cmd"], "");
        assert!(proj.get("samples").is_none());
        assert!(proj.get("payload_template").is_none());
    }

    #[test]
    fn unknown_param_names_surface_allowed() {
        let err = preflight(
            &spec_catalog(),
            "render_text",
            &params(json!({"text": "hi", "extra": 1})),
        )
        .unwrap_err();
        assert!(err.message.contains("NOT sent"));
        assert!(err.message.contains("unknown param"));
        assert!(err.message.contains("extra"));
    }

    #[test]
    fn params_example_validates_for_common_shapes() {
        for cmd in spec_catalog() {
            let proj = invoke_projection(&cmd);
            if let Some(ex) = proj.get("params_example") {
                let map = ex
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                assert!(
                    preflight(std::slice::from_ref(&cmd), &cmd.name, &map).is_ok(),
                    "example for {} must validate: {ex}",
                    cmd.name
                );
            } else {
                // no-params commands still project without an example object
                // only when there is nothing useful to show — reboot has {}.
                assert!(
                    cmd.parameters.is_empty() || validated_params_example(&cmd).is_none(),
                    "{}",
                    cmd.name
                );
            }
        }
        let reboot = &spec_catalog()[0];
        let proj = invoke_projection(reboot);
        assert_eq!(proj["params_example"], json!({}));
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

    #[test]
    fn cjk_spaces_quotes_newlines_are_valid_text_params() {
        let text = "你好 world\nline2 \"quoted\"";
        assert!(preflight(
            &spec_catalog(),
            "render_text",
            &params(json!({ "text": text }))
        )
        .is_ok());
    }
}
