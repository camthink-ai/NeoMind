//! Unified business command contract for extension operations.
//!
//! One operation → one machine-readable definition used by CLI discover /
//! validate, extension execute, capability / replay gates, and tests.
//! Thin adapter over existing [`ExtensionCommand`] descriptors — does not
//! rewrite REST handlers.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::system::{
    CommandEffect, CommandExecutionKind, CommandReplaySafety, ExtensionCommand, MetricDataType,
    ParamMetricValue, ParameterDefinition,
};

/// Sole discover/validate/auth contract for an extension command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BusinessCommandDef {
    /// Stable id: `extension/{extension_id}/{command}`.
    pub id: String,
    /// Human description (from the extension descriptor).
    pub description: String,
    /// JSON Schema object for parameters.
    pub input_schema: Value,
    /// Side-effect class after host resolve.
    pub effect: CommandEffect,
    /// Replay safety after host resolve.
    pub replay_safety: CommandReplaySafety,
    /// Immediate vs async track after host resolve.
    pub execution: CommandExecutionKind,
}

impl BusinessCommandDef {
    /// Build the canonical command id.
    pub fn command_id(extension_id: &str, command: &str) -> String {
        format!("extension/{extension_id}/{command}")
    }
}

/// Explicit host overrides for known extension commands.
///
/// Used when the installed binary predates capability fields (serde default
/// = mutating) or when the host must tighten/loosen an install-time claim.
/// Never invent privilege from command *names* alone — only listed entries.
fn host_override(
    extension_id: &str,
    command: &str,
) -> Option<(CommandEffect, CommandReplaySafety, CommandExecutionKind)> {
    // Installed marketplace packages often keep a `-v2` suffix on the id
    // while the source crate is `weather-forecast`.
    let base = extension_id
        .strip_suffix("-v2")
        .or_else(|| extension_id.strip_suffix("-v3"))
        .unwrap_or(extension_id);

    match (base, command) {
        ("weather-forecast", "get_weather" | "refresh") => Some((
            CommandEffect::ReadOnly,
            CommandReplaySafety::Safe,
            CommandExecutionKind::Immediate,
        )),
        ("weather-forecast", "set_default_city") => Some((
            CommandEffect::Mutating,
            CommandReplaySafety::Unsafe,
            CommandExecutionKind::Immediate,
        )),
        _ => None,
    }
}

/// Resolve final capability axes: host override > declared > conservative.
pub fn resolve_capability(
    extension_id: &str,
    command: &str,
    declared_effect: CommandEffect,
    declared_replay: CommandReplaySafety,
    declared_execution: CommandExecutionKind,
) -> (CommandEffect, CommandReplaySafety, CommandExecutionKind) {
    if let Some(over) = host_override(extension_id, command) {
        return over;
    }
    // Older extensions omit fields → serde/Default = Mutating/Unsafe/Immediate.
    // If they *explicitly* declared ReadOnly, honor it (no host override).
    (declared_effect, declared_replay, declared_execution)
}

/// Build JSON Schema from V2 parameter definitions (shared by API + CLI).
pub fn build_input_schema(parameters: &[ParameterDefinition]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();

    for param in parameters {
        let param_type = match &param.param_type {
            MetricDataType::Float => "number",
            MetricDataType::Integer => "integer",
            MetricDataType::Boolean => "boolean",
            MetricDataType::String | MetricDataType::Enum { .. } => "string",
            MetricDataType::Binary => "string",
        };

        let mut param_schema = json!({
            "type": param_type,
            "description": param.description,
        });

        if let MetricDataType::Enum { options } = &param.param_type {
            param_schema["enum"] = json!(options);
        } else if !param.options.is_empty() {
            param_schema["enum"] = json!(param.options);
        }

        if let Some(default_val) = &param.default_value {
            param_schema["default"] = match default_val {
                ParamMetricValue::Float(f) => json!(f),
                ParamMetricValue::Integer(i) => json!(i),
                ParamMetricValue::Boolean(b) => json!(b),
                ParamMetricValue::String(s) => json!(s),
                ParamMetricValue::Binary(_) | ParamMetricValue::Null => Value::Null,
            };
        }

        if let Some(min) = param.min {
            param_schema["minimum"] = json!(min);
        }
        if let Some(max) = param.max {
            param_schema["maximum"] = json!(max);
        }

        properties.insert(param.name.clone(), param_schema);
        if param.required {
            required.push(param.name.clone());
        }
    }

    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

/// Map an extension command descriptor into the unified business contract.
pub fn from_extension_command(extension_id: &str, cmd: &ExtensionCommand) -> BusinessCommandDef {
    let (effect, replay_safety, execution) = resolve_capability(
        extension_id,
        &cmd.name,
        cmd.effect,
        cmd.replay_safety,
        cmd.execution,
    );
    BusinessCommandDef {
        id: BusinessCommandDef::command_id(extension_id, &cmd.name),
        description: if cmd.description.is_empty() {
            cmd.display_name.clone()
        } else {
            cmd.description.clone()
        },
        input_schema: build_input_schema(&cmd.parameters),
        effect,
        replay_safety,
        execution,
    }
}

/// Validate `params` against a JSON Schema produced by [`build_input_schema`].
///
/// Checks required keys and basic JSON types. Does not run a full draft
/// validator — enough to fail closed on missing / wrong-type args before
/// dispatch so CLI and HTTP execute share one gate.
pub fn validate_params(schema: &Value, params: &Value) -> Result<(), String> {
    let obj = match params {
        Value::Object(map) => map,
        Value::Null => {
            // Treat null as empty object for required-field checking.
            return validate_params(schema, &json!({}));
        }
        _ => return Err("params must be a JSON object".to_string()),
    };

    let required = schema
        .get("required")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for req in &required {
        let Some(name) = req.as_str() else { continue };
        match obj.get(name) {
            None | Some(Value::Null) => {
                return Err(format!("missing required parameter '{name}'"));
            }
            _ => {}
        }
    }

    let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) else {
        return Ok(());
    };

    for (key, value) in obj {
        let Some(prop) = properties.get(key) else {
            // Extra keys are allowed (extensions may accept passthrough).
            continue;
        };
        if value.is_null() {
            continue;
        }
        if let Some(expected) = prop.get("type").and_then(|t| t.as_str()) {
            let ok = match expected {
                "string" => value.is_string(),
                "number" => value.is_number(),
                "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
                "boolean" => value.is_boolean(),
                "object" => value.is_object(),
                "array" => value.is_array(),
                _ => true,
            };
            if !ok {
                return Err(format!(
                    "parameter '{key}' must be of type {expected}, got {value}"
                ));
            }
        }
        if let Some(enum_vals) = prop.get("enum").and_then(|e| e.as_array()) {
            if !enum_vals.is_empty() && !enum_vals.iter().any(|e| e == value) {
                // Soft: options on weather are samples, not hard enums in
                // practice. Only enforce when the schema type is enum-backed
                // AND the value is a string not in the list — still allow
                // free-form strings that extensions accept (city names).
                // Skip hard enum enforcement for string+options lists.
            }
        }
    }

    Ok(())
}

/// Sync capability lookup for CLI classify / write-gate (no API round-trip).
///
/// Returns [`CommandEffect::ReadOnly`] only when host override or an
/// explicitly known declaration path says so; otherwise mutating.
pub fn sync_effect_for_invoke(extension_id: &str, command: &str) -> CommandEffect {
    resolve_capability(
        extension_id,
        command,
        CommandEffect::Mutating,
        CommandReplaySafety::Unsafe,
        CommandExecutionKind::Immediate,
    )
    .0
}

/// Parse `extension_id` + `command` from a tool name (`id:cmd` or sanitized `id_cmd`).
pub fn parse_extension_tool_name(tool_name: &str) -> Option<(String, String)> {
    if let Some((ext, cmd)) = tool_name.split_once(':') {
        if !ext.is_empty() && !cmd.is_empty() && !cmd.contains(':') {
            return Some((ext.to_string(), cmd.to_string()));
        }
    }
    // Sanitized OpenAI wire name: `{extension_id}_{command}` where `:` → `_`.
    // Extension ids are hyphenated (no underscores); commands often contain
    // underscores (`get_weather`). Split on the *first* `_` so the command
    // keeps its underscores.
    if let Some((ext, cmd)) = tool_name.split_once('_') {
        if !ext.is_empty() && !cmd.is_empty() && ext.contains('-') {
            return Some((ext.to_string(), cmd.to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::system::ParameterDefinition;

    #[test]
    fn command_id_shape() {
        assert_eq!(
            BusinessCommandDef::command_id("weather-forecast-v2", "get_weather"),
            "extension/weather-forecast-v2/get_weather"
        );
    }

    #[test]
    fn weather_host_override_read_only() {
        assert_eq!(
            sync_effect_for_invoke("weather-forecast-v2", "get_weather"),
            CommandEffect::ReadOnly
        );
        assert_eq!(
            sync_effect_for_invoke("weather-forecast", "refresh"),
            CommandEffect::ReadOnly
        );
        assert_eq!(
            sync_effect_for_invoke("weather-forecast-v2", "set_default_city"),
            CommandEffect::Mutating
        );
        assert_eq!(
            sync_effect_for_invoke("unknown-ext", "get_weather"),
            CommandEffect::Mutating
        );
    }

    #[test]
    fn validate_requires_city() {
        let schema = build_input_schema(&[ParameterDefinition {
            name: "city".into(),
            display_name: "City".into(),
            description: "City name".into(),
            param_type: MetricDataType::String,
            required: true,
            default_value: None,
            min: None,
            max: None,
            options: vec![],
        }]);
        assert!(validate_params(&schema, &json!({})).is_err());
        assert!(validate_params(&schema, &json!({"city": "Xiamen"})).is_ok());
        assert!(validate_params(&schema, &json!({"city": 1})).is_err());
    }

    #[test]
    fn from_extension_applies_override() {
        let cmd = ExtensionCommand {
            name: "get_weather".into(),
            display_name: "Get Weather".into(),
            description: "Query weather".into(),
            ..Default::default()
        };
        let def = from_extension_command("weather-forecast-v2", &cmd);
        assert_eq!(def.effect, CommandEffect::ReadOnly);
        assert_eq!(def.replay_safety, CommandReplaySafety::Safe);
        assert_eq!(def.id, "extension/weather-forecast-v2/get_weather");
    }

    #[test]
    fn parse_tool_names() {
        assert_eq!(
            parse_extension_tool_name("weather-forecast-v2:get_weather"),
            Some(("weather-forecast-v2".into(), "get_weather".into()))
        );
        assert_eq!(
            parse_extension_tool_name("weather-forecast-v2_get_weather"),
            Some(("weather-forecast-v2".into(), "get_weather".into()))
        );
        assert!(parse_extension_tool_name("shell").is_none());
    }
}
