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
/// Returns `None` when generation is unreliable (core checks reject the
/// candidate) so callers can show defs only. Discovery and error feedback
/// both go through this — never surface an example that would fail invoke.
pub fn validated_params_example(cmd: &CommandSpec) -> Option<Value> {
    let example = params_example_from_spec(cmd);
    let map = match &example {
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<HashMap<_, _>>(),
        _ => return None,
    };
    check_command_params(cmd, &map).ok().map(|_| example)
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
/// Attaches `params_example` only when [`validated_params_example`] succeeds
/// (same gate as CLI discovery). When generation is unreliable, callers get
/// parameter defs + constraints only — never an example that would fail
/// invoke.
pub fn command_schema_json(cmd: &CommandSpec) -> Value {
    let mut schema = json!({
        "command": cmd.name,
        "display_name": cmd.display_name,
        "description": cmd.description,
        "parameters": cmd.parameters.iter().map(param_schema_json).collect::<Vec<_>>(),
        "input_contract": "--params takes this command's param object (host wraps the protocol envelope)",
    });
    if let Some(example) = validated_params_example(cmd) {
        if let Some(map) = schema.as_object_mut() {
            map.insert("params_example".into(), example);
        }
    }
    schema
}

/// Format the optional example clause for error messages.
///
/// When no validated example exists, point at the hint's parameter
/// constraints instead of inventing a failing sample.
fn example_clause(cmd: &CommandSpec) -> String {
    match validated_params_example(cmd) {
        Some(ex) => format!(
            " Example: {}.",
            serde_json::to_string(&ex).unwrap_or_else(|_| "{}".into())
        ),
        None => {
            " See parameter definitions and constraints in the hint (no reliable params_example)."
                .to_string()
        }
    }
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
///
/// Param checks share [`check_command_params`] with example validation so
/// discovery and error feedback never disagree about a sample's legality.
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
    let envelope = looks_like_protocol_envelope(params, &known);

    match check_command_params(spec, params) {
        Ok(()) => Ok(()),
        Err(reason) => {
            let mut message = format!(
                "Command '{}' was NOT sent: {}. --params takes the command param object \
                 (host wraps the protocol).{}",
                command,
                reason,
                example_clause(spec)
            );
            if envelope {
                message.push_str(
                    " Input looks like a protocol envelope {\"cmd\":...,\"params\":{...}} — \
                     pass the inner command params at the top level instead. Do not auto-unwrap.",
                );
            }
            Err(PreflightError::with_schema(
                message,
                &command_schema_json(spec),
            ))
        }
    }
}

/// Core param checks for one known command — shared by invoke preflight and
/// example self-check. Returns a short reason string on failure (no schema /
/// example enrichment — callers attach that after).
fn check_command_params(spec: &CommandSpec, params: &HashMap<String, Value>) -> Result<(), String> {
    let known: std::collections::HashSet<&str> =
        spec.parameters.iter().map(|p| p.name.as_str()).collect();

    let missing: Vec<&str> = spec
        .parameters
        .iter()
        .filter(|p| p.required && !params.contains_key(&p.name) && p.default.is_none())
        .map(|p| p.name.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "missing required top-level params: {}",
            missing.join(", ")
        ));
    }

    let unknown: Vec<&str> = params
        .keys()
        .filter(|k| !known.contains(k.as_str()))
        .map(|k| k.as_str())
        .collect();
    if !unknown.is_empty() {
        let mut names: Vec<&str> = known.iter().copied().collect();
        names.sort_unstable();
        return Err(format!(
            "unknown param(s): {}. Allowed: {}",
            unknown.join(", "),
            if names.is_empty() {
                "(none)".to_string()
            } else {
                names.join(", ")
            }
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
            return Err(msg);
        }
        if let Some(allowed) = &p.allowed_values {
            if !allowed.is_empty() && !allowed.iter().any(|a| json_values_loosely_equal(a, raw)) {
                return Err(format!(
                    "param '{}' value {} is not in allowed_values {:?}",
                    p.name, raw, allowed
                ));
            }
        }
        if p.data_type == "integer" || p.data_type == "float" {
            let num = raw.as_f64().or_else(|| raw.as_i64().map(|i| i as f64));
            if let Some(n) = num {
                if let Some(min) = p.min {
                    if n < min {
                        return Err(format!(
                            "param '{}' value {} is below minimum {}{}",
                            p.name,
                            n,
                            min,
                            p.unit.as_deref().unwrap_or("")
                        ));
                    }
                }
                if let Some(max) = p.max {
                    if n > max {
                        return Err(format!(
                            "param '{}' value {} is above maximum {}{}",
                            p.name,
                            n,
                            max,
                            p.unit.as_deref().unwrap_or("")
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

    /// When the heuristic sample fails the same core checks invoke uses
    /// (e.g. integer generator truncates min=0.5 → 0, which violates the
    /// floor), discovery and error feedback both omit params_example.
    #[test]
    fn unreliable_example_omitted_from_discovery_and_error_feedback() {
        let cmd = CommandSpec {
            name: "set_gain".into(),
            display_name: "Set Gain".into(),
            description: "Gain".into(),
            parameters: vec![ParamSpec {
                name: "gain".into(),
                display_name: "Gain".into(),
                data_type: "integer".into(),
                required: true,
                description: String::new(),
                allowed_values: None,
                min: Some(0.5),
                max: Some(2.0),
                default: None,
                unit: None,
            }],
        };
        // Generator would emit 0 (min as i64) — that fails check_command_params.
        let raw = params_example_from_spec(&cmd);
        assert_eq!(raw["gain"], 0);
        assert!(
            check_command_params(&cmd, &params(raw.clone())).is_err(),
            "raw generator sample must fail core checks"
        );
        assert!(
            validated_params_example(&cmd).is_none(),
            "validated gate must drop the illegal sample"
        );

        let proj = invoke_projection(&cmd);
        assert!(
            proj.get("params_example").is_none(),
            "discovery omits example: {proj}"
        );
        assert!(proj["parameters"][0]["min"].as_f64() == Some(0.5));
        assert!(proj["parameters"][0]["max"].as_f64() == Some(2.0));

        let err = preflight(std::slice::from_ref(&cmd), "set_gain", &HashMap::new()).unwrap_err();
        assert!(err.message.contains("NOT sent"));
        assert!(
            !err.message.contains("Example:"),
            "error body must not advertise a failing sample: {}",
            err.message
        );
        assert!(
            err.message.contains("no reliable params_example")
                || err.message.contains("parameter definitions"),
            "{}",
            err.message
        );
        let hint: Value = serde_json::from_str(err.hint.as_deref().unwrap()).unwrap();
        assert!(
            hint.get("params_example").is_none(),
            "error hint omits example: {hint}"
        );
        assert_eq!(hint["parameters"][0]["min"], 0.5);
        assert_eq!(hint["parameters"][0]["max"], 2.0);

        // Negative-only max also yields illegal 0 from the integer default.
        let neg = CommandSpec {
            name: "set_delta".into(),
            display_name: "Set Delta".into(),
            description: "Delta".into(),
            parameters: vec![ParamSpec {
                name: "delta".into(),
                display_name: "Delta".into(),
                data_type: "integer".into(),
                required: true,
                description: String::new(),
                allowed_values: None,
                min: None,
                max: Some(-1.0),
                default: None,
                unit: None,
            }],
        };
        assert!(validated_params_example(&neg).is_none());
        let hint2 = command_schema_json(&neg);
        assert!(hint2.get("params_example").is_none(), "{hint2}");
    }
}
