//! Unified business command contract for extension operations.
//!
//! Identities are separated (design review §§1–3,7):
//! - **Type contract** — `extension.type/{type}/{command}` + `contract_version`
//! - **Instance** — installed extension id (invoke target)
//! - **Operation** — `operation_id` on each prepared invoke
//!
//! Pipeline: parse → validate + explicit defaults → **canonical params** →
//! capability resolve → fingerprint / execute.
//!
//! Capability fields on descriptors are `Option<_>`: `None` = undeclared
//! (old IPC), `Some(Mutating)` = explicit claim (must not be overridden).

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

/// AI-facing access check for an extension command.
///
/// Covers the three persisted/author controls (must agree across direct tools,
/// Chat shell → CLI, and HTTP invoke/execute):
/// - extension master `enabled` (AI off for the whole extension)
/// - `disabled_commands` (per-command AI off)
/// - descriptor `ai_exposed=false` (author hides from AI)
///
/// Returns `None` when AI may invoke; `Some(reason)` when it must refuse.
/// Automation / Transform paths that are not AI-facing should not call this.
pub fn ai_command_access_denied(
    extension_ai_enabled: bool,
    disabled_commands: &[String],
    command_name: &str,
    ai_exposed: bool,
) -> Option<String> {
    if !extension_ai_enabled {
        return Some(format!(
            "extension AI is disabled (master enabled=false); cannot AI-invoke '{command_name}'"
        ));
    }
    if !ai_exposed {
        return Some(format!(
            "command '{command_name}' declares ai_exposed=false (author-hidden from AI)"
        ));
    }
    if disabled_commands.iter().any(|c| c == command_name) {
        return Some(format!(
            "command '{command_name}' is disabled for AI (disabled_commands)"
        ));
    }
    None
}

/// Keywords we fully implement (recursive).
const IMPLEMENTED_SCHEMA_KEYS: &[&str] = &[
    "type",
    "properties",
    "required",
    "default",
    "minimum",
    "maximum",
    "enum",
    "items",
    "minLength",
    "maxLength",
    "pattern",
    "additionalProperties",
];

/// Documentary keys that may appear and are ignored (not constraints).
const DOCUMENTARY_SCHEMA_KEYS: &[&str] = &["description", "title", "examples", "$comment"];

/// Type-level command definition (sole schema/capability contract).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BusinessCommandDef {
    pub id: String,
    pub command: String,
    pub contract_version: String,
    pub description: String,
    pub input_schema: Value,
    pub effect: CommandEffect,
    pub replay_safety: CommandReplaySafety,
    pub execution: CommandExecutionKind,
    pub capability_source: CapabilitySource,
}

impl BusinessCommandDef {
    pub fn type_command_id(type_base: &str, command: &str) -> String {
        format!("extension.type/{type_base}/{command}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CapabilitySource {
    Declared,
    HostCompatOverride {
        bound_version: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        bound_digest: Option<String>,
        reason: String,
    },
    ConservativeDefault,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvokeTarget {
    pub instance_id: String,
    pub type_base: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreparedInvoke {
    pub operation_id: String,
    pub target: InvokeTarget,
    pub command: String,
    pub contract: BusinessCommandDef,
    pub canonical_params: Value,
    pub param_fingerprint: String,
    /// Load-lifecycle generation bound at prepare. Execute must consume this
    /// same token — a reload mints a new one and stale prepares are refused.
    pub capability_generation: String,
}

#[derive(Debug, Clone)]
pub struct PrepareInvokeRequest<'a> {
    pub instance_id: &'a str,
    pub command: &'a ExtensionCommand,
    pub raw_params: &'a Value,
    pub extension_version: Option<&'a str>,
    pub package_digest: Option<&'a str>,
    /// Load-lifecycle generation from the **same** snapshot as `command`.
    /// Required — production must not look up "current" (that lets stale
    /// descriptors borrow a fresh token after reload).
    pub capability_generation: &'a CapabilityGeneration,
}

/// Opaque capability-cache generation. Only [`begin_instance_generation`] may mint
/// or install the current token for an instance (load / reload).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapabilityGeneration(String);

impl CapabilityGeneration {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Effective capability resolved for one command at load time.
///
/// This is the host's decision after declared effect / version-bound compat /
/// conservative default — not the raw `Option` on the descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveCommandCapability {
    pub effect: CommandEffect,
    pub replay_safety: CommandReplaySafety,
    pub execution: CommandExecutionKind,
}

/// Load-time snapshot: commands + generation + **effective** capabilities
/// share one identity. Holding an old snapshot after reload keeps the old
/// token (prepare refuses). Write-gate / CLI classify must be able to read
/// the current generation from this published snapshot — cold start must
/// not *depend* on a prior `prepare_invoke` / ExtensionTool warm-up
/// (`remember_capability` may still run on those paths for compat).
#[derive(Debug, Clone)]
pub struct ExtensionCommandSnapshot {
    pub instance_id: String,
    pub extension_version: String,
    pub package_digest: Option<String>,
    pub commands: Vec<ExtensionCommand>,
    pub capability_generation: CapabilityGeneration,
    /// Per-command effective capability resolved at this load.
    pub effective_capabilities: HashMap<String, EffectiveCommandCapability>,
}

impl ExtensionCommandSnapshot {
    /// Build a load snapshot and resolve effective capabilities for every
    /// command. Does **not** publish into the process classify cache — call
    /// [`Self::publish_capabilities`] from register.
    pub fn build(
        instance_id: impl Into<String>,
        extension_version: impl Into<String>,
        package_digest: Option<String>,
        commands: Vec<ExtensionCommand>,
        capability_generation: CapabilityGeneration,
    ) -> Self {
        let instance_id = instance_id.into();
        let extension_version = extension_version.into();
        let mut effective_capabilities = HashMap::with_capacity(commands.len());
        for cmd in &commands {
            let def = from_extension_command(
                &instance_id,
                cmd,
                Some(extension_version.as_str()),
                package_digest.as_deref(),
            );
            effective_capabilities.insert(
                cmd.name.clone(),
                EffectiveCommandCapability {
                    effect: def.effect,
                    replay_safety: def.replay_safety,
                    execution: def.execution,
                },
            );
        }
        Self {
            instance_id,
            extension_version,
            package_digest,
            commands,
            capability_generation,
            effective_capabilities,
        }
    }

    pub fn find_command(&self, name: &str) -> Option<&ExtensionCommand> {
        self.commands.iter().find(|c| c.name == name)
    }

    /// Effective effect for `command` from this snapshot (unknown → Mutating).
    pub fn effect_for(&self, command: &str) -> CommandEffect {
        self.effective_capabilities
            .get(command)
            .map(|c| c.effect)
            .unwrap_or(CommandEffect::Mutating)
    }

    /// Install this load's effective capabilities into the process-local
    /// classify cache under `capability_generation`.
    ///
    /// Called from extension **register** so CLI-only cold start can classify
    /// without a prior invoke. This is the primary publisher for load-time
    /// caps — not the exclusive writer: `prepare_invoke` and direct-tool
    /// compat may still call [`remember_capability`]; classify must not
    /// *depend* on those warm-ups.
    pub fn publish_capabilities(&self) -> Result<(), String> {
        for (name, cap) in &self.effective_capabilities {
            remember_capability(
                &self.instance_id,
                name,
                cap.effect,
                cap.replay_safety,
                cap.execution,
                &self.capability_generation,
            )?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Type-base / version-bound compat
// ---------------------------------------------------------------------------

pub fn type_base_of(instance_id: &str) -> String {
    let mut base = instance_id;
    if let Some((head, tail)) = instance_id.rsplit_once("-v") {
        if !head.is_empty() && !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            base = head;
        }
    }
    base.to_string()
}

struct CompatGrant {
    type_base: &'static str,
    command: &'static str,
    versions: &'static [&'static str],
    digests: &'static [&'static str],
    effect: CommandEffect,
    replay_safety: CommandReplaySafety,
    execution: CommandExecutionKind,
    reason: &'static str,
}

const COMPAT_GRANTS: &[CompatGrant] = &[CompatGrant {
    type_base: "weather-forecast",
    command: "get_weather",
    versions: &["2.7.7", "2.12.0", "2.14.0", "2.15.0", "2.16.0"],
    digests: &[],
    effect: CommandEffect::ReadOnly,
    replay_safety: CommandReplaySafety::Safe,
    execution: CommandExecutionKind::Immediate,
    reason: "pre-capability weather get_weather compat; remove when package declares effect",
}];

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
            && g.versions.contains(&version)
            && (g.digests.is_empty() || digest.map(|d| g.digests.contains(&d)).unwrap_or(false))
    })
}

/// Resolve capability.
///
/// - `Some(_)` on any axis = **explicit declaration** (including explicit
///   Mutating/Unsafe/Immediate) — host must not relax via compat.
/// - All `None` = undeclared → version-bound compat may apply → else conservative.
pub fn resolve_capability(
    type_base: &str,
    command: &str,
    declared_effect: Option<CommandEffect>,
    declared_replay: Option<CommandReplaySafety>,
    declared_execution: Option<CommandExecutionKind>,
    extension_version: Option<&str>,
    package_digest: Option<&str>,
) -> (
    CommandEffect,
    CommandReplaySafety,
    CommandExecutionKind,
    CapabilitySource,
) {
    let any_declared =
        declared_effect.is_some() || declared_replay.is_some() || declared_execution.is_some();
    if any_declared {
        return (
            declared_effect.unwrap_or(CommandEffect::Mutating),
            declared_replay.unwrap_or(CommandReplaySafety::Unsafe),
            declared_execution.unwrap_or(CommandExecutionKind::Immediate),
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
// Schema build + recursive validate + normalize
// ---------------------------------------------------------------------------

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

fn classify_schema_key(key: &str) -> SchemaKeyClass {
    if IMPLEMENTED_SCHEMA_KEYS.contains(&key) {
        SchemaKeyClass::Implemented
    } else if DOCUMENTARY_SCHEMA_KEYS.contains(&key) {
        SchemaKeyClass::Documentary
    } else {
        SchemaKeyClass::Unsupported
    }
}

#[derive(PartialEq)]
enum SchemaKeyClass {
    Implemented,
    Documentary,
    Unsupported,
}

const SCALAR_TYPES: &[&str] = &["string", "number", "integer", "boolean"];
const ALL_TYPES: &[&str] = &["object", "array", "string", "number", "integer", "boolean"];

/// Keywords that require an explicit string `type` (no silent skip).
const CONSTRAINT_KEYS: &[&str] = &[
    "enum",
    "minLength",
    "maxLength",
    "pattern",
    "minimum",
    "maximum",
    "properties",
    "items",
    "additionalProperties",
    "required",
];

/// Recursively reject unsupported keywords and invalid keyword shapes.
pub fn assert_supported_schema(schema: &Value) -> Result<(), String> {
    assert_supported_schema_at(schema, "")
}

fn assert_supported_schema_at(schema: &Value, path: &str) -> Result<(), String> {
    let Some(obj) = schema.as_object() else {
        return Err(format!(
            "schema at '{path}' must be a JSON object (got {schema})"
        ));
    };
    for key in obj.keys() {
        match classify_schema_key(key) {
            SchemaKeyClass::Unsupported => {
                return Err(format!(
                    "unsupported JSON Schema keyword '{key}' at '{path}' (fail closed); \
                     implemented: {}; documentary (ignored): {}",
                    IMPLEMENTED_SCHEMA_KEYS.join(", "),
                    DOCUMENTARY_SCHEMA_KEYS.join(", ")
                ));
            }
            SchemaKeyClass::Implemented | SchemaKeyClass::Documentary => {}
        }
    }

    // Keyword value-type checks (fail closed — never coerce).
    if let Some(t) = obj.get("type") {
        match t {
            Value::String(s) => {
                if !ALL_TYPES.contains(&s.as_str()) {
                    return Err(format!(
                        "unsupported schema type '{s}' at '{path}' (fail closed)"
                    ));
                }
            }
            Value::Array(_) => {
                return Err(format!(
                    "schema type unions (array form) unsupported at '{path}' (fail closed)"
                ));
            }
            other => {
                return Err(format!(
                    "schema 'type' at '{path}' must be a string, got {other}"
                ));
            }
        }
    }

    let type_str = obj.get("type").and_then(|t| t.as_str());
    let has_constraint = CONSTRAINT_KEYS.iter().any(|k| obj.contains_key(*k));
    if has_constraint && type_str.is_none() {
        return Err(format!(
            "schema at '{path}' has constraints but no string 'type' (fail closed)"
        ));
    }

    if let Some(props) = obj.get("properties") {
        let Some(map) = props.as_object() else {
            return Err(format!(
                "'properties' at '{path}' must be an object (fail closed)"
            ));
        };
        if type_str != Some("object") {
            return Err(format!(
                "'properties' requires type=object at '{path}' (fail closed)"
            ));
        }
        for (pname, prop) in map {
            let child = if path.is_empty() {
                format!("properties.{pname}")
            } else {
                format!("{path}.properties.{pname}")
            };
            assert_supported_schema_at(prop, &child)?;
        }
    }

    if let Some(items) = obj.get("items") {
        if type_str != Some("array") {
            return Err(format!(
                "'items' requires type=array at '{path}' (fail closed)"
            ));
        }
        if !items.is_object() {
            return Err(format!(
                "'items' at '{path}' must be a schema object (fail closed)"
            ));
        }
        let child = if path.is_empty() {
            "items".to_string()
        } else {
            format!("{path}.items")
        };
        assert_supported_schema_at(items, &child)?;
    }

    if let Some(ap) = obj.get("additionalProperties") {
        if !ap.is_boolean() {
            return Err(format!(
                "'additionalProperties' at '{path}' must be a boolean (got {ap}); \
                 schema objects / strings are unsupported (fail closed)"
            ));
        }
        if type_str != Some("object") {
            return Err(format!(
                "'additionalProperties' requires type=object at '{path}' (fail closed)"
            ));
        }
    }

    if let Some(req) = obj.get("required") {
        let Some(arr) = req.as_array() else {
            return Err(format!(
                "'required' at '{path}' must be an array of strings (fail closed)"
            ));
        };
        if arr.iter().any(|v| !v.is_string()) {
            return Err(format!(
                "'required' at '{path}' must contain only strings (fail closed)"
            ));
        }
    }

    if let Some(en) = obj.get("enum") {
        let Some(arr) = en.as_array() else {
            return Err(format!("'enum' at '{path}' must be an array (fail closed)"));
        };
        if arr.is_empty() {
            return Err(format!(
                "'enum' at '{path}' must be non-empty (fail closed)"
            ));
        }
    }

    for key in ["minLength", "maxLength"] {
        if let Some(v) = obj.get(key) {
            if v.as_u64().is_none() {
                return Err(format!(
                    "'{key}' at '{path}' must be a non-negative integer (fail closed)"
                ));
            }
            if type_str != Some("string") {
                return Err(format!(
                    "'{key}' requires type=string at '{path}' (fail closed)"
                ));
            }
        }
    }
    if let Some(v) = obj.get("pattern") {
        if !v.is_string() {
            return Err(format!(
                "'pattern' at '{path}' must be a string (fail closed)"
            ));
        }
        if type_str != Some("string") {
            return Err(format!(
                "'pattern' requires type=string at '{path}' (fail closed)"
            ));
        }
    }
    for key in ["minimum", "maximum"] {
        if let Some(v) = obj.get(key) {
            if !v.is_number() {
                return Err(format!(
                    "'{key}' at '{path}' must be a number (fail closed)"
                ));
            }
            if !matches!(type_str, Some("number") | Some("integer")) {
                return Err(format!(
                    "'{key}' requires type=number|integer at '{path}' (fail closed)"
                ));
            }
        }
    }

    Ok(())
}

/// Validate raw params and apply explicit schema defaults → canonical value.
pub fn validate_and_normalize(schema: &Value, raw: &Value) -> Result<Value, String> {
    assert_supported_schema(schema)?;
    normalize_value(schema, raw, "params")
}

fn check_enum(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(enum_vals) = schema.get("enum") {
        let Some(arr) = enum_vals.as_array() else {
            return Err(format!("'{path}' schema enum is not an array"));
        };
        if !arr.iter().any(|e| e == value) {
            return Err(format!("'{path}' must be one of {arr:?}, got {value}"));
        }
    }
    Ok(())
}

fn normalize_value(schema: &Value, raw: &Value, path: &str) -> Result<Value, String> {
    let expected_type = match schema.get("type") {
        None => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(Value::Array(_)) => {
            return Err(format!(
                "schema type unions unsupported at '{path}' (fail closed)"
            ));
        }
        Some(other) => {
            return Err(format!(
                "schema 'type' at '{path}' must be a string, got {other}"
            ));
        }
    };

    // Top-level null → empty object only when schema type is object.
    if matches!(expected_type, Some("object")) && raw.is_null() && path == "params" {
        let out = normalize_object(schema, &Map::new(), path)?;
        check_enum(schema, &out, path)?;
        return Ok(out);
    }

    let out = match expected_type {
        Some("object") => {
            let obj = raw
                .as_object()
                .ok_or_else(|| format!("'{path}' must be a JSON object, got {raw}"))?;
            normalize_object(schema, obj, path)?
        }
        Some("array") => {
            let arr = raw
                .as_array()
                .ok_or_else(|| format!("'{path}' must be a JSON array, got {raw}"))?;
            normalize_array(schema, arr, path)?
        }
        Some(t) if SCALAR_TYPES.contains(&t) => {
            if raw.is_null() {
                return Err(format!(
                    "'{path}' is null but schema type is '{t}' (null not allowed)"
                ));
            }
            validate_scalar(schema, raw, t, path)?;
            raw.clone()
        }
        Some(other) => {
            return Err(format!(
                "unsupported schema type '{other}' at '{path}' (fail closed)"
            ));
        }
        None => {
            // Documentary-only schemas (no type, no constraints) may pass through.
            // Constraints without type are rejected in assert_supported_schema.
            raw.clone()
        }
    };
    check_enum(schema, &out, path)?;
    Ok(out)
}

fn normalize_object(schema: &Value, raw: &Map<String, Value>, path: &str) -> Result<Value, String> {
    let properties = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    // Strict: only boolean additionalProperties (validated in assert).
    let additional = match schema.get("additionalProperties") {
        None => true,
        Some(Value::Bool(b)) => *b,
        Some(other) => {
            return Err(format!(
                "'additionalProperties' at '{path}' must be boolean, got {other}"
            ));
        }
    };

    let mut obj = raw.clone();

    // Defaults for missing keys only (explicit null is not "missing").
    for (name, prop) in &properties {
        if !obj.contains_key(name) {
            if let Some(default) = prop.get("default") {
                if !default.is_null() {
                    obj.insert(name.clone(), default.clone());
                }
            }
        }
    }

    if !additional {
        for key in obj.keys() {
            if !properties.contains_key(key) {
                return Err(format!(
                    "unknown parameter '{key}' at '{path}' (additionalProperties=false)"
                ));
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
                return Err(format!("missing required parameter '{name}' at '{path}'"));
            }
            _ => {}
        }
    }

    let mut out = Map::new();
    for (key, value) in &obj {
        let child_path = format!("{path}.{key}");
        if let Some(prop) = properties.get(key) {
            if value.is_null() {
                return Err(format!(
                    "'{child_path}' is null but property has a typed schema (null not allowed)"
                ));
            }
            out.insert(key.clone(), normalize_value(prop, value, &child_path)?);
        } else {
            out.insert(key.clone(), value.clone());
        }
    }
    Ok(Value::Object(out))
}

fn normalize_array(schema: &Value, raw: &[Value], path: &str) -> Result<Value, String> {
    let item_schema = schema.get("items").ok_or_else(|| {
        format!("array schema at '{path}' missing 'items' (fail closed — cannot validate elements)")
    })?;
    let mut out = Vec::with_capacity(raw.len());
    for (i, item) in raw.iter().enumerate() {
        let child = format!("{path}[{i}]");
        if item.is_null() {
            return Err(format!(
                "'{child}' is null (null array elements not allowed)"
            ));
        }
        out.push(normalize_value(item_schema, item, &child)?);
    }
    Ok(Value::Array(out))
}

fn validate_scalar(
    schema: &Value,
    value: &Value,
    expected: &str,
    path: &str,
) -> Result<(), String> {
    let ok = match expected {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        other => {
            return Err(format!(
                "unsupported schema type '{other}' at '{path}' (fail closed)"
            ));
        }
    };
    if !ok {
        return Err(format!("'{path}' must be of type {expected}, got {value}"));
    }
    if expected == "string" {
        let s = value.as_str().unwrap_or("");
        if let Some(min_l) = schema.get("minLength").and_then(|v| v.as_u64()) {
            if (s.chars().count() as u64) < min_l {
                return Err(format!(
                    "'{path}' violates minLength={min_l} (got length {})",
                    s.chars().count()
                ));
            }
        }
        if let Some(max_l) = schema.get("maxLength").and_then(|v| v.as_u64()) {
            if (s.chars().count() as u64) > max_l {
                return Err(format!(
                    "'{path}' violates maxLength={max_l} (got length {})",
                    s.chars().count()
                ));
            }
        }
        if let Some(pat) = schema.get("pattern").and_then(|v| v.as_str()) {
            let re = regex_lite_match(pat, s)
                .map_err(|e| format!("invalid pattern '{pat}' at '{path}': {e}"))?;
            if !re {
                return Err(format!("'{path}' does not match pattern '{pat}'"));
            }
        }
    }
    if let Some(min) = schema.get("minimum").and_then(|v| v.as_f64()) {
        if let Some(n) = value.as_f64() {
            if n < min {
                return Err(format!("'{path}' must be >= {min}"));
            }
        }
    }
    if let Some(max) = schema.get("maximum").and_then(|v| v.as_f64()) {
        if let Some(n) = value.as_f64() {
            if n > max {
                return Err(format!("'{path}' must be <= {max}"));
            }
        }
    }
    Ok(())
}

fn regex_lite_match(pattern: &str, text: &str) -> Result<bool, String> {
    match regex::Regex::new(pattern) {
        Ok(re) => Ok(re.is_match(text)),
        Err(e) => Err(e.to_string()),
    }
}

pub fn validate_params(schema: &Value, params: &Value) -> Result<(), String> {
    validate_and_normalize(schema, params).map(|_| ())
}

fn fingerprint_params(params: &Value) -> String {
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
    let s = serde_json::to_string(&sort_value(params)).unwrap_or_default();
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
    format!("op_{}", uuid::Uuid::new_v4())
}

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

    // Required token from the caller's load snapshot — never "current".
    remember_capability(
        req.instance_id,
        &req.command.name,
        contract.effect,
        contract.replay_safety,
        contract.execution,
        req.capability_generation,
    )?;

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
        capability_generation: req.capability_generation.as_str().to_string(),
    })
}

// ---------------------------------------------------------------------------
// Load-lifecycle capability cache (write-gate / CLI classify)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct CachedCap {
    effect: CommandEffect,
    replay: CommandReplaySafety,
    #[allow(dead_code)]
    execution: CommandExecutionKind,
}

struct CapCache {
    /// instance_id → current load-lifecycle generation token.
    generation: HashMap<String, CapabilityGeneration>,
    /// `{instance}\0{command}\0{generation}` → capability.
    caps: HashMap<String, CachedCap>,
}

fn cap_cache() -> &'static Mutex<CapCache> {
    static CACHE: OnceLock<Mutex<CapCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(CapCache {
            generation: HashMap::new(),
            caps: HashMap::new(),
        })
    })
}

fn versioned_key(instance_id: &str, command: &str, generation: &str) -> String {
    format!("{instance_id}\0{command}\0{generation}")
}

/// Mint and install a **new** generation for `instance_id` (load / reload).
/// Clears prior caps. Callers may only *carry* this token afterward.
pub fn begin_instance_generation(instance_id: &str) -> CapabilityGeneration {
    let token = CapabilityGeneration(format!("gen_{}", uuid::Uuid::new_v4()));
    if let Ok(mut guard) = cap_cache().lock() {
        let prefix = format!("{instance_id}\0");
        guard.caps.retain(|k, _| !k.starts_with(&prefix));
        guard
            .generation
            .insert(instance_id.to_string(), token.clone());
    }
    token
}

pub fn current_instance_generation(instance_id: &str) -> Option<CapabilityGeneration> {
    cap_cache()
        .lock()
        .ok()
        .and_then(|g| g.generation.get(instance_id).cloned())
}

/// Test-only: mint a generation without going through registry register.
#[cfg(test)]
pub fn test_begin_instance_generation(instance_id: &str) -> CapabilityGeneration {
    begin_instance_generation(instance_id)
}

fn ensure_generation_current(
    instance_id: &str,
    generation: &CapabilityGeneration,
) -> Result<(), String> {
    let guard = cap_cache()
        .lock()
        .map_err(|_| "capability cache poisoned".to_string())?;
    match guard.generation.get(instance_id) {
        Some(cur) if cur == generation => Ok(()),
        Some(_) => Err(format!(
            "stale capability generation for '{instance_id}' (instance was reloaded)"
        )),
        None => Err(format!(
            "no active capability generation for '{instance_id}' (unloaded or not registered)"
        )),
    }
}

/// Remember capability for `(instance, command)` under an already-active
/// generation. **Does not** install or change the current generation — stale
/// tokens are rejected so an old ExtensionTool cannot revive privileges.
pub fn remember_capability(
    instance_id: &str,
    command: &str,
    effect: CommandEffect,
    replay: CommandReplaySafety,
    execution: CommandExecutionKind,
    generation: &CapabilityGeneration,
) -> Result<(), String> {
    ensure_generation_current(instance_id, generation)?;
    if let Ok(mut guard) = cap_cache().lock() {
        // Re-check under write lock.
        match guard.generation.get(instance_id) {
            Some(cur) if cur == generation => {}
            Some(_) => {
                return Err(format!(
                    "stale capability generation for '{instance_id}' (instance was reloaded)"
                ));
            }
            None => {
                return Err(format!(
                    "no active capability generation for '{instance_id}' (unloaded or not registered)"
                ));
            }
        }
        guard.caps.insert(
            versioned_key(instance_id, command, generation.as_str()),
            CachedCap {
                effect,
                replay,
                execution,
            },
        );
    }
    Ok(())
}

/// Drop all cached capability for an instance (unload / reload / upgrade).
pub fn invalidate_instance_capability(instance_id: &str) {
    if let Ok(mut guard) = cap_cache().lock() {
        guard.generation.remove(instance_id);
        let prefix = format!("{instance_id}\0");
        guard.caps.retain(|k, _| !k.starts_with(&prefix));
    }
}

/// Sync effect for classify: returns the **current generation's** capability
/// (normally installed at register via
/// [`ExtensionCommandSnapshot::publish_capabilities`]; `prepare_invoke` /
/// direct-tool compat may also [`remember_capability`] the same generation).
///
/// Miss / no generation / unknown command → Mutating (fail closed). Cold
/// start must not depend on a prior `prepare_invoke` or ExtensionTool
/// registration to have already warmed the cache.
pub fn sync_effect_for_invoke(instance_id: &str, command: &str) -> CommandEffect {
    if let Ok(guard) = cap_cache().lock() {
        if let Some(gen) = guard.generation.get(instance_id) {
            if let Some(c) = guard
                .caps
                .get(&versioned_key(instance_id, command, gen.as_str()))
            {
                return c.effect;
            }
        }
    }
    CommandEffect::Mutating
}

pub fn sync_replay_for_invoke(instance_id: &str, command: &str) -> CommandReplaySafety {
    if let Ok(guard) = cap_cache().lock() {
        if let Some(gen) = guard.generation.get(instance_id) {
            if let Some(c) = guard
                .caps
                .get(&versioned_key(instance_id, command, gen.as_str()))
            {
                return c.replay;
            }
        }
    }
    CommandReplaySafety::Unsafe
}

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

    fn weather_get_undeclared() -> ExtensionCommand {
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
            effect: None,
            replay_safety: None,
            execution: None,
            ..Default::default()
        }
    }

    #[test]
    fn type_and_instance_ids_are_separate() {
        let def = from_extension_command(
            "weather-forecast-v2",
            &weather_get_undeclared(),
            Some("2.14.0"),
            None,
        );
        assert_eq!(def.id, "extension.type/weather-forecast/get_weather");
        assert_eq!(def.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn declared_read_only_beats_compat() {
        let mut cmd = weather_get_undeclared();
        cmd.effect = Some(CommandEffect::ReadOnly);
        cmd.replay_safety = Some(CommandReplaySafety::Safe);
        let def = from_extension_command("other-ext", &cmd, None, None);
        assert_eq!(def.effect, CommandEffect::ReadOnly);
        assert!(matches!(def.capability_source, CapabilitySource::Declared));
    }

    #[test]
    fn explicit_mutating_on_weather_compat_version_stays_mutating() {
        // P1: Some(Mutating) is an explicit claim — must NOT be relaxed to RO.
        let mut cmd = weather_get_undeclared();
        cmd.effect = Some(CommandEffect::Mutating);
        cmd.replay_safety = Some(CommandReplaySafety::Unsafe);
        cmd.execution = Some(CommandExecutionKind::Immediate);
        let def = from_extension_command("weather-forecast-v2", &cmd, Some("2.14.0"), None);
        assert_eq!(def.effect, CommandEffect::Mutating);
        assert_eq!(def.replay_safety, CommandReplaySafety::Unsafe);
        assert!(matches!(def.capability_source, CapabilitySource::Declared));
    }

    #[test]
    fn undeclared_weather_compat_requires_version_skips_refresh() {
        let get = weather_get_undeclared();
        let no_ver = from_extension_command("weather-forecast-v2", &get, None, None);
        assert_eq!(no_ver.effect, CommandEffect::Mutating);
        assert!(matches!(
            no_ver.capability_source,
            CapabilitySource::ConservativeDefault
        ));

        let with_ver = from_extension_command("weather-forecast-v2", &get, Some("2.14.0"), None);
        assert_eq!(with_ver.effect, CommandEffect::ReadOnly);
        assert!(matches!(
            with_ver.capability_source,
            CapabilitySource::HostCompatOverride { .. }
        ));

        let refresh = ExtensionCommand {
            name: "refresh".into(),
            effect: None,
            replay_safety: None,
            execution: None,
            ..Default::default()
        };
        let r = from_extension_command("weather-forecast-v2", &refresh, Some("2.14.0"), None);
        assert_eq!(r.effect, CommandEffect::Mutating);
    }

    #[test]
    fn read_only_and_replay_independent() {
        let mut cmd = weather_get_undeclared();
        cmd.effect = Some(CommandEffect::ReadOnly);
        cmd.replay_safety = Some(CommandReplaySafety::Unsafe);
        let def = from_extension_command("x", &cmd, None, None);
        assert_eq!(def.effect, CommandEffect::ReadOnly);
        assert_eq!(def.replay_safety, CommandReplaySafety::Unsafe);
    }

    #[test]
    fn options_are_not_hard_enums() {
        let schema = build_input_schema(&weather_get_undeclared().parameters);
        assert!(schema["properties"]["city"].get("enum").is_none());
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
            .contains("unsupported"));
    }

    #[test]
    fn additional_properties_false_rejects_unknown() {
        let schema = json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "additionalProperties": false,
            "required": ["city"]
        });
        assert!(
            validate_and_normalize(&schema, &json!({"city": "Xiamen", "extra": 1}))
                .unwrap_err()
                .contains("additionalProperties")
        );
        assert!(validate_and_normalize(&schema, &json!({"city": "Xiamen"})).is_ok());
    }

    #[test]
    fn nested_object_and_array_items_validated() {
        let schema = json!({
            "type": "object",
            "properties": {
                "loc": {
                    "type": "object",
                    "properties": {"lat": {"type": "number"}, "lon": {"type": "number"}},
                    "required": ["lat", "lon"],
                    "additionalProperties": false
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string", "minLength": 1}
                }
            },
            "required": ["loc"]
        });
        assert!(validate_and_normalize(
            &schema,
            &json!({"loc": {"lat": 1.0, "lon": 2.0}, "tags": ["a"]})
        )
        .is_ok());
        assert!(
            validate_and_normalize(&schema, &json!({"loc": {"lat": 1.0}}))
                .unwrap_err()
                .contains("required")
        );
        assert!(validate_and_normalize(
            &schema,
            &json!({"loc": {"lat": 1.0, "lon": 2.0}, "tags": [""]})
        )
        .unwrap_err()
        .contains("minLength"));
        assert!(validate_and_normalize(
            &schema,
            &json!({"loc": {"lat": 1.0, "lon": 2.0, "alt": 3}})
        )
        .unwrap_err()
        .contains("additionalProperties"));
    }

    #[test]
    fn null_on_typed_optional_rejected() {
        let schema = json!({
            "type": "object",
            "properties": {"city": {"type": "string"}}
        });
        assert!(validate_and_normalize(&schema, &json!({"city": null}))
            .unwrap_err()
            .contains("null"));
    }

    #[test]
    fn string_pattern_and_min_length() {
        let schema = json!({
            "type": "object",
            "properties": {
                "code": {"type": "string", "minLength": 2, "pattern": "^[A-Z]+$"}
            },
            "required": ["code"]
        });
        assert!(validate_and_normalize(&schema, &json!({"code": "AB"})).is_ok());
        assert!(validate_and_normalize(&schema, &json!({"code": "A"}))
            .unwrap_err()
            .contains("minLength"));
        assert!(validate_and_normalize(&schema, &json!({"code": "ab"}))
            .unwrap_err()
            .contains("pattern"));
    }

    #[test]
    fn silent_constraint_min_items_fails_closed() {
        let schema = json!({
            "type": "object",
            "properties": {
                "tags": {"type": "array", "items": {"type": "string"}, "minItems": 1}
            }
        });
        assert!(validate_and_normalize(&schema, &json!({"tags": []}))
            .unwrap_err()
            .contains("unsupported"));
    }

    #[test]
    fn enum_without_type_fails_closed() {
        let schema = json!({"enum": ["safe"]});
        let err = validate_and_normalize(&schema, &json!("other")).unwrap_err();
        assert!(
            err.contains("no string 'type'") || err.contains("constraints"),
            "got: {err}"
        );
    }

    #[test]
    fn type_union_array_form_fails_closed() {
        let schema = json!({
            "type": ["string", "null"],
            "enum": ["safe"]
        });
        assert!(validate_and_normalize(&schema, &json!("safe"))
            .unwrap_err()
            .contains("unions"));
    }

    #[test]
    fn object_enum_is_enforced() {
        let schema = json!({
            "type": "object",
            "enum": [{"mode": "safe"}],
            "properties": {"mode": {"type": "string"}},
            "required": ["mode"]
        });
        assert!(validate_and_normalize(&schema, &json!({"mode": "safe"})).is_ok());
        assert!(validate_and_normalize(&schema, &json!({"mode": "unsafe"}))
            .unwrap_err()
            .contains("one of"));
    }

    #[test]
    fn additional_properties_string_false_fails_closed() {
        let schema = json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "additionalProperties": "false"
        });
        let err = validate_and_normalize(&schema, &json!({"city": "Xiamen", "x": 1})).unwrap_err();
        assert!(err.contains("boolean"), "got: {err}");
    }

    #[test]
    fn capability_generation_token_not_rewritable_by_stale_call() {
        invalidate_instance_capability("cache-test-ext");
        let gen1 = begin_instance_generation("cache-test-ext");
        remember_capability(
            "cache-test-ext",
            "get_weather",
            CommandEffect::ReadOnly,
            CommandReplaySafety::Safe,
            CommandExecutionKind::Immediate,
            &gen1,
        )
        .unwrap();
        assert_eq!(
            sync_effect_for_invoke("cache-test-ext", "get_weather"),
            CommandEffect::ReadOnly
        );

        // Reload → new generation; old token cannot restore RO.
        let gen2 = begin_instance_generation("cache-test-ext");
        assert_ne!(gen1, gen2);
        assert_eq!(
            sync_effect_for_invoke("cache-test-ext", "get_weather"),
            CommandEffect::Mutating
        );
        let err = remember_capability(
            "cache-test-ext",
            "get_weather",
            CommandEffect::ReadOnly,
            CommandReplaySafety::Safe,
            CommandExecutionKind::Immediate,
            &gen1,
        )
        .unwrap_err();
        assert!(err.contains("stale"), "got: {err}");
        assert_eq!(
            sync_effect_for_invoke("cache-test-ext", "get_weather"),
            CommandEffect::Mutating
        );

        remember_capability(
            "cache-test-ext",
            "get_weather",
            CommandEffect::Mutating,
            CommandReplaySafety::Unsafe,
            CommandExecutionKind::Immediate,
            &gen2,
        )
        .unwrap();
        invalidate_instance_capability("cache-test-ext");
        assert_eq!(
            sync_effect_for_invoke("cache-test-ext", "get_weather"),
            CommandEffect::Mutating
        );
        assert!(remember_capability(
            "cache-test-ext",
            "get_weather",
            CommandEffect::ReadOnly,
            CommandReplaySafety::Safe,
            CommandExecutionKind::Immediate,
            &gen2,
        )
        .unwrap_err()
        .contains("no active"));
    }

    #[test]
    fn stale_prepare_after_unload_reload_cannot_restore() {
        // Unique instance; type_base still weather-forecast via `-vNNN` strip.
        let id = "weather-forecast-v801";
        invalidate_instance_capability(id);
        let gen_old = begin_instance_generation(id);
        let cmd = weather_get_undeclared();
        prepare_invoke(PrepareInvokeRequest {
            instance_id: id,
            command: &cmd,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
            capability_generation: &gen_old,
        })
        .unwrap();
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::ReadOnly
        );

        // Simulate unload then reload with a new generation (same package version).
        invalidate_instance_capability(id);
        let gen_new = begin_instance_generation(id);
        assert_ne!(gen_old, gen_new);

        let err = prepare_invoke(PrepareInvokeRequest {
            instance_id: id,
            command: &cmd,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
            capability_generation: &gen_old,
        })
        .unwrap_err();
        assert!(err.contains("stale"), "got: {err}");
        // New generation still has no RO grant from the stale call.
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::Mutating
        );
    }

    #[test]
    fn prepare_sets_uuid_operation_and_canonical() {
        let id = "weather-forecast-v802";
        invalidate_instance_capability(id);
        let gen = begin_instance_generation(id);
        let cmd = weather_get_undeclared();
        let prep = prepare_invoke(PrepareInvokeRequest {
            instance_id: id,
            command: &cmd,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
            capability_generation: &gen,
        })
        .unwrap();
        assert!(prep.operation_id.starts_with("op_"));
        assert!(uuid::Uuid::parse_str(prep.operation_id.trim_start_matches("op_")).is_ok());
        assert_eq!(prep.canonical_params["city"], "Xiamen");
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::ReadOnly
        );
    }

    #[test]
    fn cross_entry_capability_matrix() {
        let id_w = "weather-forecast-v803";
        let id_m = "mystery-ext-v803";
        invalidate_instance_capability(id_w);
        invalidate_instance_capability(id_m);
        let gen_w = begin_instance_generation(id_w);
        let gen_m = begin_instance_generation(id_m);
        let get = weather_get_undeclared();
        let ro = prepare_invoke(PrepareInvokeRequest {
            instance_id: id_w,
            command: &get,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
            capability_generation: &gen_w,
        })
        .unwrap();
        assert_eq!(ro.contract.effect, CommandEffect::ReadOnly);

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
            effect: None,
            replay_safety: None,
            execution: None,
            ..Default::default()
        };
        let wr = prepare_invoke(PrepareInvokeRequest {
            instance_id: id_w,
            command: &set,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
            capability_generation: &gen_w,
        })
        .unwrap();
        assert_eq!(wr.contract.effect, CommandEffect::Mutating);

        let mystery = ExtensionCommand {
            name: "do_thing".into(),
            effect: None,
            replay_safety: None,
            execution: None,
            ..Default::default()
        };
        let unk = prepare_invoke(PrepareInvokeRequest {
            instance_id: id_m,
            command: &mystery,
            raw_params: &json!({}),
            extension_version: Some("1.0.0"),
            package_digest: None,
            capability_generation: &gen_m,
        })
        .unwrap();
        assert_eq!(unk.contract.effect, CommandEffect::Mutating);
        assert_ne!(ro.operation_id, wr.operation_id);
    }

    #[test]
    fn stale_descriptor_after_reload_cannot_borrow_new_generation() {
        // Acquire old command snapshot + gen, reload, continue prepare with
        // the old snapshot — must refuse and must not pollute the new cache.
        let id = "weather-forecast-v804";
        invalidate_instance_capability(id);
        let gen_old = begin_instance_generation(id);
        let old_cmd = weather_get_undeclared(); // undeclared → compat RO at 2.14.0

        prepare_invoke(PrepareInvokeRequest {
            instance_id: id,
            command: &old_cmd,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
            capability_generation: &gen_old,
        })
        .unwrap();
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::ReadOnly
        );

        // Reload: new generation (same package version string).
        invalidate_instance_capability(id);
        let gen_new = begin_instance_generation(id);
        assert_ne!(gen_old, gen_new);

        // Old request keeps old descriptor + old token.
        let err = prepare_invoke(PrepareInvokeRequest {
            instance_id: id,
            command: &old_cmd,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
            capability_generation: &gen_old,
        })
        .unwrap_err();
        assert!(
            err.contains("stale") || err.contains("no active"),
            "got: {err}"
        );
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::Mutating,
            "stale prepare must not write RO into the new generation"
        );

        // New snapshot can warm the new generation independently.
        prepare_invoke(PrepareInvokeRequest {
            instance_id: id,
            command: &old_cmd,
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
            capability_generation: &gen_new,
        })
        .unwrap();
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::ReadOnly
        );
    }

    #[test]
    fn parse_tool_names() {
        assert_eq!(
            parse_extension_tool_name("weather-forecast-v2:get_weather"),
            Some(("weather-forecast-v2".into(), "get_weather".into()))
        );
    }

    #[test]
    fn ai_access_allows_when_all_flags_open() {
        assert!(ai_command_access_denied(true, &[], "get_weather", true).is_none());
    }

    #[test]
    fn ai_access_denies_master_off() {
        let r = ai_command_access_denied(false, &[], "get_weather", true).unwrap();
        assert!(r.contains("master enabled=false"), "{r}");
    }

    #[test]
    fn ai_access_denies_disabled_command() {
        let disabled = vec!["get_weather".into()];
        let r = ai_command_access_denied(true, &disabled, "get_weather", true).unwrap();
        assert!(r.contains("disabled_commands"), "{r}");
        assert!(ai_command_access_denied(true, &disabled, "other", true).is_none());
    }

    #[test]
    fn ai_access_denies_author_hidden() {
        let r = ai_command_access_denied(true, &[], "secret", false).unwrap();
        assert!(r.contains("ai_exposed=false"), "{r}");
    }

    /// NM-235: CLI-only cold start — first RO classify after register publish
    /// (no prepare_invoke, no ExtensionTool remember).
    #[test]
    fn snapshot_publish_cold_start_ro_without_prepare_or_tool() {
        let id = "weather-forecast-v835";
        invalidate_instance_capability(id);
        // Generation alone (no publish) stays Mutating — proves classify does
        // not invent RO from the name.
        let gen = begin_instance_generation(id);
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::Mutating
        );

        let snap = ExtensionCommandSnapshot::build(
            id,
            "2.14.0",
            None,
            vec![weather_get_undeclared()],
            gen,
        );
        assert_eq!(snap.effect_for("get_weather"), CommandEffect::ReadOnly);
        snap.publish_capabilities().unwrap();

        // First classify after publish — no prepare / tool warm-up.
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::ReadOnly
        );
        // Unknown command on a published instance stays fail-closed.
        assert_eq!(
            sync_effect_for_invoke(id, "totally_unknown_cmd"),
            CommandEffect::Mutating
        );
        invalidate_instance_capability(id);
    }

    /// NM-235: after reload + new snapshot publish, first RO classify is RO
    /// again without depending on prior invoke/browse.
    #[test]
    fn snapshot_publish_survives_reload_without_prior_invoke() {
        let id = "weather-forecast-v836";
        invalidate_instance_capability(id);

        let gen1 = begin_instance_generation(id);
        ExtensionCommandSnapshot::build(id, "2.14.0", None, vec![weather_get_undeclared()], gen1)
            .publish_capabilities()
            .unwrap();
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::ReadOnly
        );

        // Reload: new generation clears prior caps.
        let gen2 = begin_instance_generation(id);
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::Mutating,
            "reload must clear old snapshot grants"
        );

        // Fresh publish of the new load snapshot — still no prepare.
        ExtensionCommandSnapshot::build(id, "2.14.0", None, vec![weather_get_undeclared()], gen2)
            .publish_capabilities()
            .unwrap();
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::ReadOnly
        );
        invalidate_instance_capability(id);
    }

    /// NM-235: write / undeclared-unknown stay Mutating after snapshot publish.
    #[test]
    fn snapshot_publish_keeps_write_and_unknown_mutating() {
        let id = "weather-forecast-v837";
        invalidate_instance_capability(id);
        let gen = begin_instance_generation(id);
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
            effect: None,
            replay_safety: None,
            execution: None,
            ..Default::default()
        };
        let mystery = ExtensionCommand {
            name: "do_thing".into(),
            effect: None,
            replay_safety: None,
            execution: None,
            ..Default::default()
        };
        let snap = ExtensionCommandSnapshot::build(
            id,
            "2.14.0",
            None,
            vec![weather_get_undeclared(), set, mystery],
            gen,
        );
        assert_eq!(snap.effect_for("get_weather"), CommandEffect::ReadOnly);
        assert_eq!(snap.effect_for("set_default_city"), CommandEffect::Mutating);
        assert_eq!(snap.effect_for("do_thing"), CommandEffect::Mutating);
        snap.publish_capabilities().unwrap();

        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::ReadOnly
        );
        assert_eq!(
            sync_effect_for_invoke(id, "set_default_city"),
            CommandEffect::Mutating
        );
        assert_eq!(
            sync_effect_for_invoke(id, "do_thing"),
            CommandEffect::Mutating
        );
        invalidate_instance_capability(id);
    }

    /// NM-235: classification must not depend on prior browse/list/execute.
    #[test]
    fn snapshot_classify_independent_of_prepare_history() {
        let id = "weather-forecast-v838";
        invalidate_instance_capability(id);
        let gen = begin_instance_generation(id);
        ExtensionCommandSnapshot::build(
            id,
            "2.14.0",
            None,
            vec![weather_get_undeclared()],
            gen.clone(),
        )
        .publish_capabilities()
        .unwrap();

        // Classify BEFORE any prepare_invoke — still RO.
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::ReadOnly
        );

        // A prepare afterward must not be required for the grant; re-check.
        prepare_invoke(PrepareInvokeRequest {
            instance_id: id,
            command: &weather_get_undeclared(),
            raw_params: &json!({"city": "Xiamen"}),
            extension_version: Some("2.14.0"),
            package_digest: None,
            capability_generation: &gen,
        })
        .unwrap();
        assert_eq!(
            sync_effect_for_invoke(id, "get_weather"),
            CommandEffect::ReadOnly
        );
        invalidate_instance_capability(id);
    }
}
