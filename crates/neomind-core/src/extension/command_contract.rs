//! Unified business command contract for extension operations.
//!
//! Identities are separated (design review §§1–3,7):
//! - **Type contract** — `extension.type/{type}/{command}` + `contract_version`
//! - **Instance** — installed extension id (invoke target)
//! - **Operation** — `operation_id` on each prepared invoke (stores contract
//!   version + target); Phase 2 owns full ledger lifecycle
//!
//! Pipeline: parse → validate + explicit defaults → **canonical params** →
//! capability resolve → fingerprint / execute.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::system::{
    CommandEffect, CommandExecutionKind, CommandReplaySafety, ExtensionCommand, MetricDataType,
    ParamMetricValue, ParameterDefinition,
};

/// Contract schema version stamped on every prepared invoke.
pub const CONTRACT_VERSION: &str = "1";

/// Supported JSON Schema keywords for extension command params.
/// Anything else that is critical for correctness fails closed.
const SUPPORTED_SCHEMA_KEYS: &[&str] = &[
    "type",
    "properties",
    "required",
    "description",
    "default",
    "minimum",
    "maximum",
    "enum",
    "items",
];

/// Unsupported keywords that must fail closed (not silently ignore).
const CRITICAL_UNSUPPORTED_KEYS: &[&str] = &[
    "$ref",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
    "dependentSchemas",
    "patternProperties",
];

/// Type-level command definition (sole schema/capability contract).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BusinessCommandDef {
    /// Type-level id: `extension.type/{type_base}/{command}`.
    pub id: String,
    /// Command name on the type.
    pub command: String,
    /// Contract schema version.
    pub contract_version: String,
    /// Human description.
    pub description: String,
    /// JSON Schema (supported subset only).
    pub input_schema: Value,
    /// Side-effect class after resolve.
    pub effect: CommandEffect,
    /// Replay safety after resolve (independent of effect).
    pub replay_safety: CommandReplaySafety,
    /// Immediate vs async hint.
    pub execution: CommandExecutionKind,
    /// Where effective capability came from.
    pub capability_source: CapabilitySource,
}

impl BusinessCommandDef {
    /// Type-level command id (not an instance id, not an operation id).
    pub fn type_command_id(type_base: &str, command: &str) -> String {
        format!("extension.type/{type_base}/{command}")
    }
}

/// Provenance of the effective capability axes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CapabilitySource {
    /// Extension (or SDK default) declaration as loaded.
    Declared,
    /// Temporary host compat grant — version/digest bound.
    HostCompatOverride {
        bound_version: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        bound_digest: Option<String>,
        reason: String,
    },
    /// Missing metadata → conservative Mutating/Unsafe.
    ConservativeDefault,
}

/// Concrete invoke target (installed instance).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvokeTarget {
    /// Installed extension instance id (e.g. `weather-forecast-v2`).
    pub instance_id: String,
    /// Type base used in type-level ids (hyphen form without marketplace `-vN`).
    pub type_base: String,
    /// Installed package version when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_version: Option<String>,
    /// Package digest when known (sha256 of .nep / binary).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_digest: Option<String>,
}

/// Result of the shared prepare pipeline — same object for auth + execute.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreparedInvoke {
    /// New logical operation id for this submission (Phase 1 hook).
    pub operation_id: String,
    pub target: InvokeTarget,
    pub command: String,
    pub contract: BusinessCommandDef,
    /// Canonical params (defaults applied; same set for auth and execute).
    pub canonical_params: Value,
    /// Stable fingerprint of canonical params (content assist; ≠ operation_id).
    pub param_fingerprint: String,
}

/// Inputs for [`prepare_invoke`].
#[derive(Debug, Clone)]
pub struct PrepareInvokeRequest<'a> {
    pub instance_id: &'a str,
    pub command: &'a ExtensionCommand,
    pub raw_params: &'a Value,
    pub extension_version: Option<&'a str>,
    pub package_digest: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// Type-base / version-bound compat (temporary; prefer declared capability)
// ---------------------------------------------------------------------------

/// Strip marketplace `-vN` suffix for type-base identity.
pub fn type_base_of(instance_id: &str) -> String {
    let mut base = instance_id;
    // Only strip a trailing `-v` + digits (weather-forecast-v2 → weather-forecast).
    if let Some((head, tail)) = instance_id.rsplit_once("-v") {
        if !head.is_empty() && !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            base = head;
        }
    }
    base.to_string()
}

/// Version-bound temporary compat grants.
///
/// Host may **grant** read_only only for listed (type_base, command, version)
/// rows with explicit provenance. Never grant from command name alone.
/// `refresh` is intentionally absent — read_only ≠ name heuristics.
///
/// Prefer extension-declared `effect`/`replay_safety`. Re-verify on upgrade:
/// a version not listed here does not inherit the grant.
struct CompatGrant {
    type_base: &'static str,
    command: &'static str,
    /// Exact extension versions that may receive this grant.
    versions: &'static [&'static str],
    /// Optional package digests (any match). Empty = version-only bind.
    digests: &'static [&'static str],
    effect: CommandEffect,
    replay_safety: CommandReplaySafety,
    execution: CommandExecutionKind,
    reason: &'static str,
}

/// Known pre-capability weather builds still shipping without SDK fields.
/// Digests left empty until install records carry package hashes.
const COMPAT_GRANTS: &[CompatGrant] = &[
    CompatGrant {
        type_base: "weather-forecast",
        command: "get_weather",
        // Common installed marketplace versions observed locally / docs.
        versions: &["2.7.7", "2.12.0", "2.14.0", "2.15.0", "2.16.0"],
        digests: &[],
        effect: CommandEffect::ReadOnly,
        replay_safety: CommandReplaySafety::Safe,
        execution: CommandExecutionKind::Immediate,
        reason: "pre-capability weather get_weather compat; remove when package declares effect",
    },
];

fn compat_grant(
    type_base: &str,
    command: &str,
    version: Option<&str>,
    digest: Option<&str>,
) -> Option<&'static CompatGrant> {
    let version = version?;
    COMPAT_GRANTS.iter().find(|g| {
        g.type_base == type_base
            && g.command == command
            && g.versions.iter().any(|v| *v == version)
            && (g.digests.is_empty()
                || digest
                    .map(|d| g.digests.iter().any(|x| *x == d))
                    .unwrap_or(false))
    })
}

/// True when the loaded descriptor looks like an explicit non-default claim.
fn has_explicit_declaration(
    effect: CommandEffect,
    replay: CommandReplaySafety,
    execution: CommandExecutionKind,
) -> bool {
    !matches!(
        (effect, replay, execution),
        (
            CommandEffect::Mutating,
            CommandReplaySafety::Unsafe,
            CommandExecutionKind::Immediate
        )
    )
}

/// Resolve effective capability: declared (if non-default) > version-bound
/// compat grant > conservative default. Host does not invent name-only grants.
pub fn resolve_capability(
    type_base: &str,
    command: &str,
    declared_effect: CommandEffect,
    declared_replay: CommandReplaySafety,
    declared_execution: CommandExecutionKind,
    extension_version: Option<&str>,
    package_digest: Option<&str>,
) -> (
    CommandEffect,
    CommandReplaySafety,
    CommandExecutionKind,
    CapabilitySource,
) {
    if has_explicit_declaration(declared_effect, declared_replay, declared_execution) {
        return (
            declared_effect,
            declared_replay,
            declared_execution,
            CapabilitySource::Declared,
        );
    }

    if let Some(g) = compat_grant(type_base, command, extension_version, package_digest) {
        return (
            g.effect,
            g.replay_safety,
            g.execution,
            CapabilitySource::HostCompatOverride {
                bound_version: extension_version.unwrap_or("").to_string(),
                bound_digest: package_digest.map(str::to_string),
                reason: g.reason.to_string(),
            },
        );
    }

    (
        CommandEffect::Mutating,
        CommandReplaySafety::Unsafe,
        CommandExecutionKind::Immediate,
        CapabilitySource::ConservativeDefault,
    )
}

// ---------------------------------------------------------------------------
// Schema build + validate + normalize → canonical params
// ---------------------------------------------------------------------------

/// Build JSON Schema from V2 parameter definitions.
///
/// `ParameterDefinition.options` are **samples** for LLM hints, not hard
/// enums — only [`MetricDataType::Enum`] emits `enum`.
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

/// Reject schemas that use critical unsupported keywords.
pub fn assert_supported_schema(schema: &Value) -> Result<(), String> {
    let Some(obj) = schema.as_object() else {
        return Err("input_schema must be a JSON object".into());
    };
    for key in obj.keys() {
        if CRITICAL_UNSUPPORTED_KEYS.contains(&key.as_str()) {
            return Err(format!(
                "unsupported critical JSON Schema keyword '{key}' (fail closed); \
                 supported subset: {}",
                SUPPORTED_SCHEMA_KEYS.join(", ")
            ));
        }
        // Unknown non-critical keys (e.g. title) are ignored with no grant.
        let _ = SUPPORTED_SCHEMA_KEYS.contains(&key.as_str());
    }
    if let Some(props) = obj.get("properties").and_then(|p| p.as_object()) {
        for (pname, prop) in props {
            let Some(pobj) = prop.as_object() else {
                return Err(format!("property '{pname}' schema must be an object"));
            };
            for key in pobj.keys() {
                if CRITICAL_UNSUPPORTED_KEYS.contains(&key.as_str()) {
                    return Err(format!(
                        "unsupported critical JSON Schema keyword '{key}' on '{pname}'"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Validate raw params and apply explicit schema defaults → canonical object.
///
/// Auth and execute MUST use this same canonical value (never re-default later).
pub fn validate_and_normalize(schema: &Value, raw: &Value) -> Result<Value, String> {
    assert_supported_schema(schema)?;

    let mut obj: Map<String, Value> = match raw {
        Value::Object(map) => map.clone(),
        Value::Null => Map::new(),
        _ => return Err("params must be a JSON object".into()),
    };

    let properties = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    // Apply defaults only for missing keys (not for explicit null — null
    // means the caller omitted a meaningful value and must fail required).
    for (name, prop) in &properties {
        if !obj.contains_key(name) {
            if let Some(default) = prop.get("default") {
                if !default.is_null() {
                    obj.insert(name.clone(), default.clone());
                }
            }
        }
    }

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

    for (key, value) in &obj {
        let Some(prop) = properties.get(key) else {
            // Extra keys allowed (passthrough); still part of canonical set.
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
                other => {
                    return Err(format!(
                        "unsupported schema type '{other}' on '{key}' (fail closed)"
                    ));
                }
            };
            if !ok {
                return Err(format!(
                    "parameter '{key}' must be of type {expected}, got {value}"
                ));
            }
        }
        if let Some(enum_vals) = prop.get("enum").and_then(|e| e.as_array()) {
            if !enum_vals.is_empty() && !enum_vals.iter().any(|e| e == value) {
                return Err(format!(
                    "parameter '{key}' must be one of {enum_vals:?}, got {value}"
                ));
            }
        }
        if let Some(min) = prop.get("minimum").and_then(|v| v.as_f64()) {
            if let Some(n) = value.as_f64() {
                if n < min {
                    return Err(format!("parameter '{key}' must be >= {min}"));
                }
            }
        }
        if let Some(max) = prop.get("maximum").and_then(|v| v.as_f64()) {
            if let Some(n) = value.as_f64() {
                if n > max {
                    return Err(format!("parameter '{key}' must be <= {max}"));
                }
            }
        }
    }

    Ok(Value::Object(obj))
}

/// Backward-compatible validate-only wrapper (no defaults applied to output).
pub fn validate_params(schema: &Value, params: &Value) -> Result<(), String> {
    validate_and_normalize(schema, params).map(|_| ())
}

fn fingerprint_params(params: &Value) -> String {
    // serde_json Map preserves insertion order (preserve_order feature in
    // workspace). For a stable fingerprint, serialize sorted keys.
    fn sort_value(v: &Value) -> Value {
        match v {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut out = Map::new();
                for k in keys {
                    out.insert(k.clone(), sort_value(&map[k]));
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.iter().map(sort_value).collect()),
            other => other.clone(),
        }
    }
    let sorted = sort_value(params);
    let s = serde_json::to_string(&sorted).unwrap_or_default();
    // Lightweight non-crypto fingerprint for Phase 1 content assist.
    format!("{:016x}", fnv1a64(s.as_bytes()))
}

fn fnv1a64(data: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET;
    for b in data {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn new_operation_id() -> String {
    // UUID v4-ish without extra deps: timestamp + random-ish from fingerprint seed.
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("op_{nanos:x}")
}

/// Map an extension command into the type-level business contract.
pub fn from_extension_command(
    instance_id: &str,
    cmd: &ExtensionCommand,
    extension_version: Option<&str>,
    package_digest: Option<&str>,
) -> BusinessCommandDef {
    let type_base = type_base_of(instance_id);
    let (effect, replay_safety, execution, capability_source) = resolve_capability(
        &type_base,
        &cmd.name,
        cmd.effect,
        cmd.replay_safety,
        cmd.execution,
        extension_version,
        package_digest,
    );
    BusinessCommandDef {
        id: BusinessCommandDef::type_command_id(&type_base, &cmd.name),
        command: cmd.name.clone(),
        contract_version: CONTRACT_VERSION.to_string(),
        description: if cmd.description.is_empty() {
            cmd.display_name.clone()
        } else {
            cmd.description.clone()
        },
        input_schema: build_input_schema(&cmd.parameters),
        effect,
        replay_safety,
        execution,
        capability_source,
    }
}

/// Shared prepare: validate+normalize → capability → operation identity.
pub fn prepare_invoke(req: PrepareInvokeRequest<'_>) -> Result<PreparedInvoke, String> {
    let type_base = type_base_of(req.instance_id);
    let contract = from_extension_command(
        req.instance_id,
        req.command,
        req.extension_version,
        req.package_digest,
    );
    let canonical_params = validate_and_normalize(&contract.input_schema, req.raw_params)?;
    let param_fingerprint = fingerprint_params(&canonical_params);
    let operation_id = new_operation_id();

    // Warm the classify cache so shell write-gate sees the same effect.
    remember_capability(
        req.instance_id,
        &req.command.name,
        contract.effect,
        contract.replay_safety,
        contract.execution,
        req.extension_version,
    );

    Ok(PreparedInvoke {
        operation_id,
        target: InvokeTarget {
            instance_id: req.instance_id.to_string(),
            type_base,
            extension_version: req.extension_version.map(str::to_string),
            package_digest: req.package_digest.map(str::to_string),
        },
        command: req.command.name.clone(),
        contract,
        canonical_params,
        param_fingerprint,
    })
}

// ---------------------------------------------------------------------------
// Classify-time cache (same process as ExtensionTool registration / prepare)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct CachedCap {
    effect: CommandEffect,
    replay: CommandReplaySafety,
    #[allow(dead_code)] // reserved for sync_execution classify
    execution: CommandExecutionKind,
}

fn capability_cache() -> &'static Mutex<HashMap<String, CachedCap>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedCap>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(instance_id: &str, command: &str) -> String {
    format!("{instance_id}\0{command}")
}

/// Remember resolved capability for sync classify (agent write-gate / CLI).
pub fn remember_capability(
    instance_id: &str,
    command: &str,
    effect: CommandEffect,
    replay: CommandReplaySafety,
    execution: CommandExecutionKind,
    _version: Option<&str>,
) {
    if let Ok(mut guard) = capability_cache().lock() {
        guard.insert(
            cache_key(instance_id, command),
            CachedCap {
                effect,
                replay,
                execution,
            },
        );
    }
}

/// Sync effect for CLI classify / write-gate without a full prepare.
///
/// Order: process cache (from prepare / ExtensionTool) → conservative
/// Mutating. Does **not** apply version-bound grants without a version
/// (name-only grants are forbidden).
pub fn sync_effect_for_invoke(instance_id: &str, command: &str) -> CommandEffect {
    if let Ok(guard) = capability_cache().lock() {
        if let Some(c) = guard.get(&cache_key(instance_id, command)) {
            return c.effect;
        }
    }
    // No version at classify time → no compat grant.
    CommandEffect::Mutating
}

/// Sync replay for registry/tool paths when only instance+command are known.
pub fn sync_replay_for_invoke(instance_id: &str, command: &str) -> CommandReplaySafety {
    if let Ok(guard) = capability_cache().lock() {
        if let Some(c) = guard.get(&cache_key(instance_id, command)) {
            return c.replay;
        }
    }
    CommandReplaySafety::Unsafe
}

/// Parse `extension_id` + `command` from a tool name (`id:cmd` or sanitized `id_cmd`).
pub fn parse_extension_tool_name(tool_name: &str) -> Option<(String, String)> {
    if let Some((ext, cmd)) = tool_name.split_once(':') {
        if !ext.is_empty() && !cmd.is_empty() && !cmd.contains(':') {
            return Some((ext.to_string(), cmd.to_string()));
        }
    }
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

    fn weather_get_cmd() -> ExtensionCommand {
        ExtensionCommand {
            name: "get_weather".into(),
            display_name: "Get Weather".into(),
            description: "Query weather".into(),
            parameters: vec![ParameterDefinition {
                name: "city".into(),
                display_name: "City".into(),
                description: "City name".into(),
                param_type: MetricDataType::String,
                required: true,
                default_value: None,
                min: None,
                max: None,
                options: vec!["Beijing".into()],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn type_and_instance_ids_are_separate() {
        let def = from_extension_command("weather-forecast-v2", &weather_get_cmd(), Some("2.14.0"), None);
        assert_eq!(def.id, "extension.type/weather-forecast/get_weather");
        assert_eq!(def.command, "get_weather");
        assert_eq!(def.contract_version, CONTRACT_VERSION);
        assert_ne!(def.id, "extension/weather-forecast-v2/get_weather");
    }

    #[test]
    fn declared_read_only_beats_compat() {
        let mut cmd = weather_get_cmd();
        cmd.effect = CommandEffect::ReadOnly;
        cmd.replay_safety = CommandReplaySafety::Safe;
        let def = from_extension_command("other-ext", &cmd, None, None);
        assert_eq!(def.effect, CommandEffect::ReadOnly);
        assert!(matches!(def.capability_source, CapabilitySource::Declared));
    }

    #[test]
    fn weather_compat_requires_version_and_skips_refresh() {
        let get = weather_get_cmd();
        // No version → conservative (no name-only grant).
        let no_ver = from_extension_command("weather-forecast-v2", &get, None, None);
        assert_eq!(no_ver.effect, CommandEffect::Mutating);
        assert!(matches!(
            no_ver.capability_source,
            CapabilitySource::ConservativeDefault
        ));

        let with_ver = from_extension_command("weather-forecast-v2", &get, Some("2.14.0"), None);
        assert_eq!(with_ver.effect, CommandEffect::ReadOnly);
        assert_eq!(with_ver.replay_safety, CommandReplaySafety::Safe);
        assert!(matches!(
            with_ver.capability_source,
            CapabilitySource::HostCompatOverride { .. }
        ));

        // Unknown version does not inherit.
        let unknown_ver =
            from_extension_command("weather-forecast-v2", &get, Some("9.9.9"), None);
        assert_eq!(unknown_ver.effect, CommandEffect::Mutating);

        // refresh is NOT auto read_only.
        let refresh = ExtensionCommand {
            name: "refresh".into(),
            ..Default::default()
        };
        let r = from_extension_command("weather-forecast-v2", &refresh, Some("2.14.0"), None);
        assert_eq!(r.effect, CommandEffect::Mutating);
    }

    #[test]
    fn read_only_and_replay_are_independent_axes() {
        let mut cmd = weather_get_cmd();
        cmd.effect = CommandEffect::ReadOnly;
        cmd.replay_safety = CommandReplaySafety::Unsafe; // e.g. costly query
        let def = from_extension_command("x", &cmd, None, None);
        assert_eq!(def.effect, CommandEffect::ReadOnly);
        assert_eq!(def.replay_safety, CommandReplaySafety::Unsafe);
    }

    #[test]
    fn options_are_not_hard_enums() {
        let schema = build_input_schema(&weather_get_cmd().parameters);
        assert!(schema["properties"]["city"].get("enum").is_none());
        // Free-form city still validates.
        assert!(validate_and_normalize(&schema, &json!({"city": "Xiamen"})).is_ok());
    }

    #[test]
    fn normalize_applies_defaults_once() {
        let schema = json!({
            "type": "object",
            "properties": {
                "unit": {"type": "string", "default": "celsius"},
                "city": {"type": "string"}
            },
            "required": ["city"]
        });
        let canon = validate_and_normalize(&schema, &json!({"city": "Xiamen"})).unwrap();
        assert_eq!(canon["unit"], "celsius");
        assert_eq!(canon["city"], "Xiamen");
    }

    #[test]
    fn unsupported_schema_fails_closed() {
        let schema = json!({
            "type": "object",
            "properties": {"x": {"type": "string"}},
            "allOf": [{"required": ["x"]}]
        });
        assert!(validate_and_normalize(&schema, &json!({"x": "a"}))
            .unwrap_err()
            .contains("unsupported critical"));
    }

    #[test]
    fn prepare_sets_operation_and_canonical() {
        let cmd = weather_get_cmd();
        let prep = prepare_invoke(PrepareInvokeRequest {
            instance_id: "weather-forecast-v2",
            command: &cmd,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
        })
        .unwrap();
        assert!(prep.operation_id.starts_with("op_"));
        assert_eq!(prep.target.instance_id, "weather-forecast-v2");
        assert_eq!(prep.target.type_base, "weather-forecast");
        assert_eq!(prep.contract.id, "extension.type/weather-forecast/get_weather");
        assert_eq!(prep.canonical_params["city"], "Xiamen");
        assert!(!prep.param_fingerprint.is_empty());
        // Cache warmed for classify.
        assert_eq!(
            sync_effect_for_invoke("weather-forecast-v2", "get_weather"),
            CommandEffect::ReadOnly
        );
    }

    #[test]
    fn sync_without_cache_is_mutating() {
        // Unique instance so cache from other tests does not leak.
        assert_eq!(
            sync_effect_for_invoke("no-such-ext-xyz", "get_weather"),
            CommandEffect::Mutating
        );
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
    }

    /// Cross-entry: same prepare for read-only / write / unknown-capability.
    #[test]
    fn cross_entry_capability_matrix() {
        // Read-only via version-bound compat (pre-declared packages).
        let get = weather_get_cmd();
        let ro = prepare_invoke(PrepareInvokeRequest {
            instance_id: "weather-forecast-v2",
            command: &get,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
        })
        .unwrap();
        assert_eq!(ro.contract.effect, CommandEffect::ReadOnly);
        assert!(matches!(
            ro.contract.capability_source,
            CapabilitySource::HostCompatOverride { .. }
        ));

        // Write (mutating) — set_default_city never gets a compat grant.
        let set = ExtensionCommand {
            name: "set_default_city".into(),
            parameters: vec![ParameterDefinition {
                name: "city".into(),
                display_name: "City".into(),
                description: "".into(),
                param_type: MetricDataType::String,
                required: true,
                default_value: None,
                min: None,
                max: None,
                options: vec![],
            }],
            ..Default::default()
        };
        let wr = prepare_invoke(PrepareInvokeRequest {
            instance_id: "weather-forecast-v2",
            command: &set,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
        })
        .unwrap();
        assert_eq!(wr.contract.effect, CommandEffect::Mutating);
        assert!(matches!(
            wr.contract.capability_source,
            CapabilitySource::ConservativeDefault
        ));

        // Unknown capability (no declaration, no compat) → mutating.
        let mystery = ExtensionCommand {
            name: "do_thing".into(),
            ..Default::default()
        };
        let unk = prepare_invoke(PrepareInvokeRequest {
            instance_id: "mystery-ext",
            command: &mystery,
            raw_params: &json!({}),
            extension_version: Some("1.0.0"),
            package_digest: None,
        })
        .unwrap();
        assert_eq!(unk.contract.effect, CommandEffect::Mutating);
        assert!(matches!(
            unk.contract.capability_source,
            CapabilitySource::ConservativeDefault
        ));

        // Same canonical params for identical inputs (fingerprint stable).
        let again = prepare_invoke(PrepareInvokeRequest {
            instance_id: "weather-forecast-v2",
            command: &get,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
        })
        .unwrap();
        assert_eq!(ro.param_fingerprint, again.param_fingerprint);
        // New submission → new operation_id (hooks for Phase 2 ledger).
        assert_ne!(ro.operation_id, again.operation_id);
    }
}
