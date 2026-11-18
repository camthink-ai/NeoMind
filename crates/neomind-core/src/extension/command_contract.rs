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

/// Server-derived caller of an extension invoke/execute (NM-241).
///
/// Must never be taken from a request body `caller=` field — clients must not
/// self-claim admin. HTTP derives this from authenticated session + whether a
/// Chat-issued `prepared_operation_id` is present; Transform/registry in-process
/// paths use [`ExtensionCallerKind::InternalAutomation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionCallerKind {
    /// Chat write-gate path (shell → CLI → HTTP with Chat-issued prepare token).
    AiChat,
    /// Agent / API-key / CLI automation without a Chat prepare token.
    AgentRuntime,
    /// Authenticated human operator (JWT user session, not an API-key service
    /// account). Subject to normal RBAC; not the AI toggles.
    HumanAdmin,
    /// Host automation (Transform `extensions.invoke`, rules, …) that never
    /// enters the HTTP AI-policy prepare path.
    InternalAutomation,
}

impl ExtensionCallerKind {
    /// Whether `ai_enabled` / `disabled_commands` / `ai_exposed` apply.
    ///
    /// Only AI-facing kinds are bound. HumanAdmin uses RBAC; InternalAutomation
    /// is host-owned. `ai_exposed=false` still blocks AI kinds only.
    pub fn ai_policy_applies(self) -> bool {
        matches!(self, Self::AiChat | Self::AgentRuntime)
    }
}

/// AI-facing access check for an extension command.
///
/// Covers the three persisted/author controls (must agree across direct tools,
/// Chat shell → CLI, and HTTP invoke/execute for AI callers):
/// - extension master `ai_enabled` (AI off for the whole extension; distinct
///   from run/load `enabled`)
/// - `disabled_commands` (per-command AI off)
/// - descriptor `ai_exposed=false` (author hides from AI)
///
/// Returns `None` when AI may invoke; `Some(reason)` when it must refuse.
/// Call only when [`ExtensionCallerKind::ai_policy_applies`] is true —
/// HumanAdmin / InternalAutomation must not be routed through this gate.
pub fn ai_command_access_denied(
    extension_ai_enabled: bool,
    disabled_commands: &[String],
    command_name: &str,
    ai_exposed: bool,
) -> Option<String> {
    if !extension_ai_enabled {
        return Some(format!(
            "extension AI is disabled (ai_enabled=false); cannot AI-invoke '{command_name}'"
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
        // Catalog needed so Chat can prepare+authorize a real PreparedInvoke
        // (write-gate bound to the same token execute will consume).
        publish_prepareable_snapshot(self);
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

        // Only MetricDataType::Enum is a closed set. Plain `options` are
        // author suggestions / hints (weather city list) — validation still
        // accepts values outside the list (compat with free-text + hints).
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

/// Command catalog + identity needed to mint a [`PreparedInvoke`] at Chat
/// classify time (same generation register published).
#[derive(Clone)]
struct PrepareableSnapshot {
    extension_version: String,
    package_digest: Option<String>,
    commands: Vec<ExtensionCommand>,
    capability_generation: CapabilityGeneration,
}

/// Host AI visibility for one instance (mirrors ExtensionRecord toggles).
#[derive(Clone, Debug)]
struct InstanceAiVisibility {
    ai_enabled: bool,
    disabled_commands: Vec<String>,
}

impl Default for InstanceAiVisibility {
    fn default() -> Self {
        Self {
            ai_enabled: true,
            disabled_commands: Vec::new(),
        }
    }
}

struct CapCache {
    /// instance_id → current load-lifecycle generation token.
    generation: HashMap<String, CapabilityGeneration>,
    /// `{instance}\0{command}\0{generation}` → capability.
    caps: HashMap<String, CachedCap>,
    /// instance_id → prepareable catalog for the current generation.
    prepareable: HashMap<String, PrepareableSnapshot>,
    /// instance_id → Chat/Agent AI visibility (ai_enabled / disabled_commands).
    ai_visibility: HashMap<String, InstanceAiVisibility>,
}

fn cap_cache() -> &'static Mutex<CapCache> {
    static CACHE: OnceLock<Mutex<CapCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(CapCache {
            generation: HashMap::new(),
            caps: HashMap::new(),
            prepareable: HashMap::new(),
            ai_visibility: HashMap::new(),
        })
    })
}

/// Publish Chat/Agent AI visibility for an instance (same policy as
/// `extension commands` / invoke: `ai_enabled`, `disabled_commands`).
/// Call from AI toggle handlers and tool-disable bootstrap.
pub fn set_instance_ai_visibility(
    instance_id: &str,
    ai_enabled: bool,
    disabled_commands: Vec<String>,
) {
    if let Ok(mut guard) = cap_cache().lock() {
        guard.ai_visibility.insert(
            instance_id.to_string(),
            InstanceAiVisibility {
                ai_enabled,
                disabled_commands,
            },
        );
    }
}

fn command_visible_to_ai(cmd: &ExtensionCommand, vis: &InstanceAiVisibility) -> bool {
    if !vis.ai_enabled {
        return false;
    }
    if !cmd.ai_exposed {
        return false;
    }
    !vis.disabled_commands.iter().any(|c| c == &cmd.name)
}

fn versioned_key(instance_id: &str, command: &str, generation: &str) -> String {
    format!("{instance_id}\0{command}\0{generation}")
}

/// Install the load snapshot's command catalog for Chat prepare+authorize.
fn publish_prepareable_snapshot(snapshot: &ExtensionCommandSnapshot) {
    if let Ok(mut guard) = cap_cache().lock() {
        // Only publish when the generation is still current (reload race).
        match guard.generation.get(&snapshot.instance_id) {
            Some(cur) if cur == &snapshot.capability_generation => {}
            _ => return,
        }
        guard.prepareable.insert(
            snapshot.instance_id.clone(),
            PrepareableSnapshot {
                extension_version: snapshot.extension_version.clone(),
                package_digest: snapshot.package_digest.clone(),
                commands: snapshot.commands.clone(),
                capability_generation: snapshot.capability_generation.clone(),
            },
        );
    }
}

/// Default host rank budget for [`search_ai_extension_commands`] (NM-252).
pub const AI_COMMAND_SEARCH_MAX_HITS: usize = 3;
/// ~500 tokens × ~4 chars/token — purpose + required params only.
pub const AI_COMMAND_SEARCH_MAX_CHARS: usize = 2000;
/// Below this score the host returns zero hits (relevance too low).
const AI_COMMAND_SEARCH_MIN_SCORE: u32 = 15;

/// One ranked AI-visible command hit from the current registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiCommandSearchHit {
    pub extension_id: String,
    pub command: String,
    pub purpose: String,
    /// Required param name + short description (schema stays on `commands <id>`).
    pub required_params: Vec<AiCommandSearchParam>,
    pub score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiCommandSearchParam {
    pub name: String,
    pub description: String,
}

/// Ranked search result over installed AI-visible commands (NM-252).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiCommandSearchResult {
    pub query: String,
    pub scope_extension_id: Option<String>,
    pub hits: Vec<AiCommandSearchHit>,
    /// AI-visible commands considered before rank/truncate.
    pub candidates_scanned: usize,
    pub truncated: bool,
}

/// Fixed-length discovery rules for the resident capability prompt (NM-252).
///
/// Same text whether 5 or 500 extensions are installed — no per-extension
/// catalog growth. Prefer `--search` then invoke; reuse known on follow-up.
pub fn format_extension_discovery_rules() -> String {
    "\n### Extension capability discovery\n\
     - First need / unknown capability: `neomind extension commands --search \"<task>\"` \
(host ranks ≤3 AI-visible cmds with purpose + required params; zero hits allowed).\n\
     - Known id+command from this session: reuse → `neomind extension invoke <id> <command>` \
with shell `params` (skip re-search).\n\
     - Full schema when id known: `neomind extension commands <id>`.\n\
     - @ one extension: `neomind extension commands --search \"<task>\" <id>` (scoped).\n\
     - Web/announcement intent: use web tools; skip extension search.\n\
     - Uninstall/disable/reload may reject invoke — search again.\n"
        .to_string()
}

/// Search installed AI-visible commands from the current prepareable snapshot.
///
/// Filters with the same policy as catalog/invoke (`ai_enabled` /
/// `disabled_commands` / `ai_exposed`; missing AI policy → omit). Host-side
/// lexical rank only — no second index or vector DB. Returns at most
/// `max_hits` complete entries within `max_chars`; may return zero hits.
pub fn search_ai_extension_commands(
    query: &str,
    scope_extension_id: Option<&str>,
    max_hits: usize,
    max_chars: usize,
) -> AiCommandSearchResult {
    let query_trim = query.trim();
    let empty = AiCommandSearchResult {
        query: query_trim.to_string(),
        scope_extension_id: scope_extension_id.map(str::to_string),
        hits: Vec::new(),
        candidates_scanned: 0,
        truncated: false,
    };
    if query_trim.is_empty() || max_hits == 0 || max_chars == 0 {
        return empty;
    }

    let tokens = search_query_tokens(query_trim);
    if tokens.is_empty() {
        return empty;
    }

    let mut scored: Vec<AiCommandSearchHit> = {
        let Ok(guard) = cap_cache().lock() else {
            return empty;
        };
        let mut out = Vec::new();
        for (id, snap) in guard.prepareable.iter() {
            if let Some(scope) = scope_extension_id {
                if id != scope {
                    continue;
                }
            }
            let Some(vis) = guard.ai_visibility.get(id) else {
                continue;
            };
            for cmd in snap
                .commands
                .iter()
                .filter(|c| command_visible_to_ai(c, vis))
            {
                let score = score_command_against_query(id, cmd, query_trim, &tokens);
                if score < AI_COMMAND_SEARCH_MIN_SCORE {
                    continue;
                }
                let purpose = {
                    let d = cmd.description.trim();
                    if d.is_empty() {
                        cmd.display_name.trim().to_string()
                    } else {
                        d.chars().take(160).collect()
                    }
                };
                let required_params: Vec<AiCommandSearchParam> = cmd
                    .parameters
                    .iter()
                    .filter(|p| p.required)
                    .map(|p| AiCommandSearchParam {
                        name: p.name.clone(),
                        description: p.description.chars().take(80).collect(),
                    })
                    .collect();
                out.push(AiCommandSearchHit {
                    extension_id: id.clone(),
                    command: cmd.name.clone(),
                    purpose,
                    required_params,
                    score,
                });
            }
        }
        out
    };

    let candidates_scanned = scored.len();
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.extension_id.cmp(&b.extension_id))
            .then_with(|| a.command.cmp(&b.command))
    });

    // Deduplicate identical (extension, command) keeping best score.
    scored.dedup_by(|a, b| a.extension_id == b.extension_id && a.command == b.command);

    let mut hits = Vec::new();
    let mut used = 0usize;
    let mut truncated = false;
    // Envelope reserved so a later consumer can wrap JSON without blowing budget.
    let envelope = 120usize;
    let budget = max_chars.saturating_sub(envelope);

    for hit in scored.into_iter() {
        if hits.len() >= max_hits {
            truncated = true;
            break;
        }
        let approx = estimate_hit_chars(&hit);
        if used + approx > budget && !hits.is_empty() {
            truncated = true;
            break;
        }
        if used + approx > budget && hits.is_empty() {
            // First hit alone exceeds budget — still emit one truncated purpose.
            let mut slim = hit;
            slim.purpose = slim.purpose.chars().take(80).collect();
            slim.required_params.truncate(4);
            hits.push(slim);
            truncated = candidates_scanned > 1;
            break;
        }
        used += approx;
        hits.push(hit);
    }

    AiCommandSearchResult {
        query: query_trim.to_string(),
        scope_extension_id: scope_extension_id.map(str::to_string),
        hits,
        candidates_scanned,
        truncated,
    }
}

fn estimate_hit_chars(hit: &AiCommandSearchHit) -> usize {
    let mut n = hit.extension_id.len() + hit.command.len() + hit.purpose.len() + 48;
    for p in &hit.required_params {
        n += p.name.len() + p.description.len() + 8;
    }
    n
}

fn search_query_tokens(query: &str) -> Vec<String> {
    let lower = query.to_lowercase();
    let mut tokens: Vec<String> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || is_cjk(c)))
        .filter(|t| !t.is_empty() && (t.len() >= 2 || t.chars().any(is_cjk)))
        .map(|t| t.to_string())
        .collect();
    // Keep the full normalized query as a phrase when multi-token / CJK.
    let phrase: String = lower
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || is_cjk(c) {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if !phrase.is_empty() && !tokens.iter().any(|t| t == &phrase) {
        tokens.push(phrase);
    }
    tokens.sort();
    tokens.dedup();
    tokens
}

fn is_cjk(c: char) -> bool {
    matches!(
        c,
        '\u{4E00}'..='\u{9FFF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{3040}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

fn score_command_against_query(
    extension_id: &str,
    cmd: &ExtensionCommand,
    query: &str,
    tokens: &[String],
) -> u32 {
    let q = query.to_lowercase();
    let name = cmd.name.to_lowercase();
    let display = cmd.display_name.to_lowercase();
    let desc = cmd.description.to_lowercase();
    let ext = extension_id.to_lowercase();
    let name_norm = name.replace(['-', ' '], "_");
    let q_norm = q.replace(['-', ' '], "_");

    let mut score = 0u32;
    if name == q || name_norm == q_norm {
        score += 100;
    } else if name.contains(&q) || name_norm.contains(&q_norm) {
        score += 60;
    }
    if display == q {
        score += 40;
    } else if !q.is_empty() && display.contains(&q) {
        score += 25;
    }
    if !q.is_empty() && desc.contains(&q) {
        score += 35;
    }
    if !q.is_empty() && ext.contains(&q) {
        score += 30;
    }

    let mut matched = 0u32;
    for tok in tokens {
        if tok.len() < 2 && !tok.chars().any(is_cjk) {
            continue;
        }
        let mut hit = false;
        if name.contains(tok.as_str()) || name_norm.contains(&tok.replace('-', "_")) {
            score += 25;
            hit = true;
        }
        if display.contains(tok.as_str()) {
            score += 12;
            hit = true;
        }
        if desc.contains(tok.as_str()) {
            score += 15;
            hit = true;
        }
        if ext.contains(tok.as_str()) {
            score += 10;
            hit = true;
        }
        for p in &cmd.parameters {
            let pn = p.name.to_lowercase();
            let pd = p.description.to_lowercase();
            if pn.contains(tok.as_str()) || pd.contains(tok.as_str()) {
                score += if p.required { 10 } else { 5 };
                hit = true;
            }
        }
        if hit {
            matched += 1;
        }
    }
    if !tokens.is_empty() && matched as usize == tokens.len() {
        score += 20;
    }
    score
}

/// Short AI extension catalog for capability context (NM-247).
///
/// Same AI visibility as `extension commands` / invoke: `ai_enabled`,
/// `disabled_commands`, and descriptor `ai_exposed`. Instances without a
/// published AI policy snapshot are omitted (fail closed — not default-visible).
/// Byte budget counts complete entries only. Full schemas stay on-demand via
/// `neomind extension commands <id>`.
///
/// NM-252: transitional — resident prompts prefer
/// [`format_extension_discovery_rules`] + `--search` over this catalog.
pub fn format_ai_extension_catalog(max_lines: usize, max_bytes: usize) -> String {
    let entries: Vec<(String, String)> = {
        let Ok(guard) = cap_cache().lock() else {
            return String::new();
        };
        let mut out = Vec::new();
        for (id, snap) in guard.prepareable.iter() {
            // Fail closed: no AI policy snapshot yet → omit (do not default-visible).
            let Some(vis) = guard.ai_visibility.get(id).cloned() else {
                continue;
            };
            let ai_cmds: Vec<_> = snap
                .commands
                .iter()
                .filter(|c| command_visible_to_ai(c, &vis))
                .collect();
            if ai_cmds.is_empty() {
                continue;
            }
            let one_line = ai_cmds
                .first()
                .map(|c| {
                    let d = c.description.trim();
                    if d.is_empty() {
                        format!("{} command(s)", ai_cmds.len())
                    } else {
                        d.chars().take(80).collect::<String>()
                    }
                })
                .unwrap_or_else(|| "installed extension".into());
            out.push((id.clone(), one_line));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    };
    if entries.is_empty() {
        return String::new();
    }
    let header = "\n### AI Extensions\n";
    let footer = "Prefer: `neomind extension commands --search \"<task>\"` then invoke; \
known id: `commands <id>`; reuse known on follow-up.\n";
    let trunc = "- … (truncated; run `neomind extension commands --search \"…\"`)\n";
    let mut s = String::from(header);
    let mut n = 0usize;
    let mut omitted = false;
    for (id, line) in &entries {
        let entry = format!("- {id} — {line}\n");
        let next_len = s.len() + entry.len() + footer.len();
        if n >= max_lines || next_len > max_bytes {
            omitted = true;
            break;
        }
        s.push_str(&entry);
        n += 1;
    }
    if n == 0 {
        // First complete entry did not fit the budget — omit catalog rather
        // than emit a partial line.
        return String::new();
    }
    if omitted {
        if s.len() + trunc.len() + footer.len() <= max_bytes {
            s.push_str(trunc);
        }
    }
    s.push_str(footer);
    s
}

/// Build a params example that passes [`validate_and_normalize`].
///
/// Prefer author `samples`, then closed schema `enum` / default, then
/// non-Enum `options` as **hints** (sampled then validated against the
/// original schema — never hardened into enum). Empty string only when the
/// schema allows it (not invented). Returns `None` when no reliable example
/// exists — callers should point at param docs instead.
pub fn validated_params_example_for_command(cmd: &ExtensionCommand) -> Option<Value> {
    let schema = build_input_schema(&cmd.parameters);
    for sample in &cmd.samples {
        if validate_and_normalize(&schema, sample).is_ok() {
            return Some(sample.clone());
        }
    }
    let generated = params_example_from_schema_and_hints(&schema, &cmd.parameters);
    match generated {
        Some(ex) if validate_and_normalize(&schema, &ex).is_ok() => Some(ex),
        _ => None,
    }
}

fn option_hints_for_param<'a>(
    name: &str,
    parameters: &'a [ParameterDefinition],
) -> Option<&'a [String]> {
    parameters.iter().find_map(|p| {
        if p.name != name {
            return None;
        }
        // Closed Enum already lands in schema.enum; hints are for non-Enum
        // ParameterDefinition.options only.
        if matches!(p.param_type, MetricDataType::Enum { .. }) {
            return None;
        }
        if p.options.is_empty() {
            None
        } else {
            Some(p.options.as_slice())
        }
    })
}

fn params_example_from_schema_and_hints(
    schema: &Value,
    parameters: &[ParameterDefinition],
) -> Option<Value> {
    let props = schema.get("properties")?.as_object()?;
    let required: std::collections::HashSet<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    let mut obj = Map::new();
    for (key, prop) in props {
        if !required.is_empty() && !required.contains(key.as_str()) {
            continue;
        }
        let sample = if let Some(d) = prop.get("default") {
            d.clone()
        } else if let Some(en) = prop.get("enum").and_then(|e| e.as_array()) {
            en.first()?.clone()
        } else if let Some(hints) = option_hints_for_param(key, parameters) {
            json!(hints.first()?.clone())
        } else {
            match prop.get("type").and_then(|t| t.as_str()) {
                Some("string") => {
                    // Required free-form string without enum/default/hints →
                    // unreliable. Omit rather than invent "" or a city name.
                    continue;
                }
                Some("number") | Some("integer") => json!(0),
                Some("boolean") => json!(false),
                Some("array") => json!([]),
                Some("object") => json!({}),
                _ => continue,
            }
        };
        obj.insert(key.clone(), sample);
        if obj.len() >= 6 {
            break;
        }
    }
    for req in &required {
        if !obj.contains_key(*req) {
            return None;
        }
    }
    Some(Value::Object(obj))
}

/// Mint and install a **new** generation for `instance_id` (load / reload).
/// Clears prior caps, prepareable catalog, and Chat prepared tickets.
/// Callers may only *carry* this token afterward.
pub fn begin_instance_generation(instance_id: &str) -> CapabilityGeneration {
    let token = CapabilityGeneration(format!("gen_{}", uuid::Uuid::new_v4()));
    if let Ok(mut guard) = cap_cache().lock() {
        let prefix = format!("{instance_id}\0");
        guard.caps.retain(|k, _| !k.starts_with(&prefix));
        guard.prepareable.remove(instance_id);
        guard
            .generation
            .insert(instance_id.to_string(), token.clone());
    }
    clear_prepared_tickets_for_instance(instance_id);
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
        guard.prepareable.remove(instance_id);
    }
    clear_prepared_tickets_for_instance(instance_id);
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

// ---------------------------------------------------------------------------
// Chat write-gate ↔ PreparedInvoke binding (NM-240)
//
// Target path:
//   prepare → operation_id + generation + canonical params + effect + fingerprint
//     → Chat write-gate / authorize (bound to that token)
//     → consume same prepared once
//     → bound execution
//
// RO skip is only valid while holding a live authorized PreparedInvoke.
// Reload clears tickets → stale; RO→Mutating cannot skip; Mutating→RO old
// auth cannot transfer (bound to operation_id + generation + fingerprint).
// ---------------------------------------------------------------------------

struct PreparedTicket {
    prepared: PreparedInvoke,
    /// Chat write-gate (or RO skip) bound this exact token.
    chat_authorized: bool,
    consumed: bool,
    /// Shell / ExtensionTool has claimed this ticket for the current dispatch.
    claimed: bool,
}

struct PreparedTicketStore {
    /// operation_id → ticket. Tickets are bound to the classify-time dispatch
    /// that carries this id — never claimed by (instance, command, params).
    by_id: HashMap<String, PreparedTicket>,
}

fn prepared_ticket_store() -> &'static Mutex<PreparedTicketStore> {
    static STORE: OnceLock<Mutex<PreparedTicketStore>> = OnceLock::new();
    STORE.get_or_init(|| {
        Mutex::new(PreparedTicketStore {
            by_id: HashMap::new(),
        })
    })
}

fn clear_prepared_tickets_for_instance(instance_id: &str) {
    if let Ok(mut guard) = prepared_ticket_store().lock() {
        guard
            .by_id
            .retain(|_, t| t.prepared.target.instance_id != instance_id);
    }
}

fn ticket_generation_current(prepared: &PreparedInvoke) -> bool {
    current_instance_generation(&prepared.target.instance_id)
        .map(|g| g.as_str() == prepared.capability_generation)
        .unwrap_or(false)
}

/// Issue a **new** Chat-bound PreparedInvoke from the published catalog.
///
/// Always mints a fresh `operation_id`. The caller **must** carry that id on
/// the same dispatch through write-gate → execute ([`claim_prepared_by_id`]).
/// Tickets are never claimed by (instance, command, params) from a global FIFO.
pub fn prepare_and_issue_for_chat(
    instance_id: &str,
    command: &str,
    raw_params: &Value,
) -> Result<PreparedInvoke, String> {
    let snap = {
        let guard = cap_cache()
            .lock()
            .map_err(|_| "capability cache poisoned".to_string())?;
        let snap = guard.prepareable.get(instance_id).cloned().ok_or_else(|| {
            format!("no prepareable snapshot for '{instance_id}' (unloaded or not published)")
        })?;
        match guard.generation.get(instance_id) {
            Some(cur) if cur == &snap.capability_generation => {}
            Some(_) => {
                return Err(format!(
                    "stale prepareable snapshot for '{instance_id}' (instance was reloaded)"
                ));
            }
            None => {
                return Err(format!(
                    "no active capability generation for '{instance_id}'"
                ));
            }
        }
        snap
    };

    let cmd = snap
        .commands
        .iter()
        .find(|c| c.name == command)
        .ok_or_else(|| {
            format!("command '{command}' not in prepareable catalog for '{instance_id}'")
        })?;

    let prepared = prepare_invoke(PrepareInvokeRequest {
        instance_id,
        command: cmd,
        raw_params,
        extension_version: Some(snap.extension_version.as_str()),
        package_digest: snap.package_digest.as_deref(),
        capability_generation: &snap.capability_generation,
    })?;

    if let Ok(mut guard) = prepared_ticket_store().lock() {
        guard.by_id.insert(
            prepared.operation_id.clone(),
            PreparedTicket {
                prepared: prepared.clone(),
                chat_authorized: false,
                consumed: false,
                claimed: false,
            },
        );
    }
    Ok(prepared)
}

/// Bind Chat write-gate / RO-skip authorization to an issued PreparedInvoke.
///
/// Requires matching `operation_id` + `capability_generation` +
/// `param_fingerprint`. Fails when the ticket is missing, already consumed,
/// or stale after reload.
pub fn authorize_prepared_for_chat(
    operation_id: &str,
    capability_generation: &str,
    param_fingerprint: &str,
) -> Result<(), String> {
    let mut guard = prepared_ticket_store()
        .lock()
        .map_err(|_| "prepared ticket store poisoned".to_string())?;
    let ticket = guard.by_id.get_mut(operation_id).ok_or_else(|| {
        format!("unknown prepared operation '{operation_id}' (missing or cleared after reload)")
    })?;
    if ticket.consumed {
        return Err(format!(
            "prepared operation '{operation_id}' already consumed"
        ));
    }
    if !ticket_generation_current(&ticket.prepared) {
        return Err(format!(
            "stale prepared invoke '{operation_id}': capability generation expired after reload"
        ));
    }
    if ticket.prepared.capability_generation != capability_generation {
        return Err(format!(
            "prepared operation '{operation_id}' generation mismatch"
        ));
    }
    if ticket.prepared.param_fingerprint != param_fingerprint {
        return Err(format!(
            "prepared operation '{operation_id}' fingerprint mismatch (params changed)"
        ));
    }
    ticket.chat_authorized = true;
    Ok(())
}

/// Authorize an **already-issued** PreparedInvoke by id — never re-prepares.
///
/// Call after Chat write-gate Proceed for Mutating, or at RO classify.
/// Missing / consumed / stale-after-reload → Err (must re-classify).
pub fn authorize_existing_prepared(operation_id: &str) -> Result<PreparedInvoke, String> {
    let prepared = peek_prepared_invoke(operation_id).ok_or_else(|| {
        format!("stale prepared invoke '{operation_id}': missing ticket (reload or never issued)")
    })?;
    authorize_prepared_for_chat(
        operation_id,
        &prepared.capability_generation,
        &prepared.param_fingerprint,
    )?;
    peek_prepared_invoke(operation_id)
        .ok_or_else(|| format!("prepared operation '{operation_id}' disappeared during authorize"))
}

/// Claim + authorize an **already-issued** PreparedInvoke by the id this
/// dispatch carried from classify — never looks up by params / FIFO.
///
/// Same-lock checks: missing → stale; consumed → Err; **already claimed** →
/// Err; generation expired → stale. Marks `claimed` and sets Chat-authorized
/// (RO was authorized at classify; Mutating authorizes here after write-gate).
pub fn claim_prepared_by_id(operation_id: &str) -> Result<PreparedInvoke, String> {
    let mut guard = prepared_ticket_store()
        .lock()
        .map_err(|_| "prepared ticket store poisoned".to_string())?;
    let ticket = guard.by_id.get_mut(operation_id).ok_or_else(|| {
        format!(
            "stale prepared invoke '{operation_id}': missing ticket (reload or never issued);              re-classify and invoke again"
        )
    })?;
    if ticket.consumed {
        return Err(format!(
            "prepared operation '{operation_id}' already consumed"
        ));
    }
    if ticket.claimed {
        return Err(format!(
            "prepared operation '{operation_id}' already claimed by another dispatch"
        ));
    }
    if !ticket_generation_current(&ticket.prepared) {
        return Err(format!(
            "stale prepared invoke '{operation_id}': capability generation expired after reload"
        ));
    }
    ticket.claimed = true;
    ticket.chat_authorized = true;
    Ok(ticket.prepared.clone())
}

/// Claim by id and verify the ticket still matches this dispatch's
/// `(instance, command, params)`. Ownership checks run **before** marking
/// claimed so a fingerprint mismatch does not burn the ticket.
pub fn claim_prepared_for_dispatch(
    operation_id: &str,
    instance_id: &str,
    command: &str,
    raw_params: &Value,
) -> Result<PreparedInvoke, String> {
    // Peek first for ownership (no claim yet).
    let prepared = peek_prepared_invoke(operation_id).ok_or_else(|| {
        format!(
            "stale prepared invoke '{operation_id}': missing ticket (reload or never issued); \
             re-classify and invoke again"
        )
    })?;
    if prepared.target.instance_id != instance_id || prepared.command != command {
        return Err(format!(
            "prepared operation '{operation_id}' does not match '{instance_id}/{command}'"
        ));
    }
    let canonical = validate_and_normalize(&prepared.contract.input_schema, raw_params)?;
    let fp = fingerprint_params(&canonical);
    if prepared.param_fingerprint != fp {
        return Err(format!(
            "prepared operation '{operation_id}' fingerprint mismatch (params changed)"
        ));
    }
    // Same-lock claim after ownership checks pass.
    claim_prepared_by_id(operation_id)
}

/// Consume a Chat-authorized PreparedInvoke **once** for execution.
///
/// Binds `operation_id` + `capability_generation` + `param_fingerprint`.
/// Refuses unauthorized, consumed, stale, or mismatched tickets.
pub fn consume_authorized_prepared(
    operation_id: &str,
    capability_generation: &str,
    param_fingerprint: &str,
) -> Result<PreparedInvoke, String> {
    let mut guard = prepared_ticket_store()
        .lock()
        .map_err(|_| "prepared ticket store poisoned".to_string())?;
    let ticket = guard.by_id.get_mut(operation_id).ok_or_else(|| {
        format!("stale prepared invoke '{operation_id}': missing ticket (reload or never issued)")
    })?;
    if ticket.consumed {
        return Err(format!(
            "prepared operation '{operation_id}' already consumed (single-consume)"
        ));
    }
    if !ticket.chat_authorized {
        return Err(format!(
            "prepared operation '{operation_id}' was not Chat-authorized"
        ));
    }
    if !ticket_generation_current(&ticket.prepared) {
        return Err(format!(
            "stale prepared invoke '{operation_id}': capability generation expired after reload"
        ));
    }
    if ticket.prepared.capability_generation != capability_generation {
        return Err(format!(
            "prepared operation '{operation_id}' generation mismatch"
        ));
    }
    if ticket.prepared.param_fingerprint != param_fingerprint {
        return Err(format!(
            "prepared operation '{operation_id}' fingerprint mismatch (params changed)"
        ));
    }
    ticket.consumed = true;
    let prepared = ticket.prepared.clone();
    // Drop the slot so a second consume cannot resurrect it.
    guard.by_id.remove(operation_id);
    Ok(prepared)
}

/// Look up a live authorized ticket for `(instance, command, fingerprint)`.
///
/// Does not consume. Used by Chat shell to inject `prepared_operation_id`
/// into the CLI → HTTP path.
pub fn find_authorized_prepared(
    instance_id: &str,
    command: &str,
    param_fingerprint: &str,
) -> Option<PreparedInvoke> {
    let guard = prepared_ticket_store().lock().ok()?;
    guard.by_id.values().find_map(|t| {
        if t.consumed || !t.chat_authorized {
            return None;
        }
        let p = &t.prepared;
        if p.target.instance_id == instance_id
            && p.command == command
            && p.param_fingerprint == param_fingerprint
            && ticket_generation_current(p)
        {
            Some(p.clone())
        } else {
            None
        }
    })
}

/// Peek an issued (possibly unauthorized) ticket by operation id.
pub fn peek_prepared_invoke(operation_id: &str) -> Option<PreparedInvoke> {
    let guard = prepared_ticket_store().lock().ok()?;
    guard.by_id.get(operation_id).map(|t| t.prepared.clone())
}

/// HTTP/CLI execute entry: consume a Chat-authorized PreparedInvoke by id,
/// verifying instance, command, and request params still match the token.
///
/// Param drift invalidates (fingerprint mismatch). Missing/unauthorized/
/// already-consumed/stale-after-reload → Err.
pub fn consume_chat_prepared_for_execute(
    operation_id: &str,
    instance_id: &str,
    command: &str,
    raw_params: &Value,
) -> Result<PreparedInvoke, String> {
    let issued = peek_prepared_invoke(operation_id).ok_or_else(|| {
        format!("stale prepared invoke '{operation_id}': missing ticket (reload or never issued)")
    })?;
    if issued.target.instance_id != instance_id {
        return Err(format!(
            "prepared operation '{operation_id}' targets '{}' not '{instance_id}'",
            issued.target.instance_id
        ));
    }
    if issued.command != command {
        return Err(format!(
            "prepared operation '{operation_id}' is for command '{}' not '{command}'",
            issued.command
        ));
    }
    let canonical = validate_and_normalize(&issued.contract.input_schema, raw_params)?;
    let fp = fingerprint_params(&canonical);
    if fp != issued.param_fingerprint {
        return Err(format!(
            "prepared operation '{operation_id}' fingerprint mismatch (params changed)"
        ));
    }
    consume_authorized_prepared(
        operation_id,
        &issued.capability_generation,
        &issued.param_fingerprint,
    )
}

/// Result of classify + prepare for one Chat dispatch (NM-240).
///
/// `prepared_operation_id` must travel with **this** tool call through
/// write-gate → execute. Never reclaim a ticket by params from a global queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatInvokeBind {
    pub effect: CommandEffect,
    /// Issued ticket id for this classify. `None` when prepare failed
    /// (effect is fail-closed [`CommandEffect::Mutating`]).
    pub prepared_operation_id: Option<String>,
}

/// Classify + bind Chat write-gate for an extension invoke.
///
/// - Success with [`CommandEffect::ReadOnly`]: prepares, **authorizes**, returns RO + id.
/// - Success with Mutating: prepares (issued, not yet authorized), returns Mutating + id.
/// - Prepare failure: Mutating with `prepared_operation_id = None`.
///
/// Callers must carry `prepared_operation_id` on the same dispatch; execute
/// claims that id only ([`claim_prepared_by_id`]).
pub fn classify_and_bind_chat_invoke(
    instance_id: &str,
    command: &str,
    raw_params: &Value,
) -> ChatInvokeBind {
    match prepare_and_issue_for_chat(instance_id, command, raw_params) {
        Ok(prepared) => {
            let id = prepared.operation_id.clone();
            if prepared.contract.effect == CommandEffect::ReadOnly {
                if authorize_prepared_for_chat(
                    &prepared.operation_id,
                    &prepared.capability_generation,
                    &prepared.param_fingerprint,
                )
                .is_ok()
                {
                    ChatInvokeBind {
                        effect: CommandEffect::ReadOnly,
                        prepared_operation_id: Some(id),
                    }
                } else {
                    // Authorization failed (reload race) — fail closed; drop id.
                    ChatInvokeBind {
                        effect: CommandEffect::Mutating,
                        prepared_operation_id: None,
                    }
                }
            } else {
                ChatInvokeBind {
                    effect: CommandEffect::Mutating,
                    prepared_operation_id: Some(id),
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                instance_id = %instance_id,
                command = %command,
                error = %e,
                stage = "classify_prepare",
                "prepare_and_issue_for_chat failed — no PreparedInvoke ticket for this dispatch"
            );
            ChatInvokeBind {
                effect: CommandEffect::Mutating,
                prepared_operation_id: None,
            }
        }
    }
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
        assert!(r.contains("ai_enabled=false"), "{r}");
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

    #[test]
    fn caller_kind_ai_policy_binds_only_ai_surfaces() {
        assert!(ExtensionCallerKind::AiChat.ai_policy_applies());
        assert!(ExtensionCallerKind::AgentRuntime.ai_policy_applies());
        assert!(!ExtensionCallerKind::HumanAdmin.ai_policy_applies());
        assert!(!ExtensionCallerKind::InternalAutomation.ai_policy_applies());
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

    // -----------------------------------------------------------------------
    // NM-240 — Chat write-gate bound to the same PreparedInvoke
    // -----------------------------------------------------------------------

    fn publish_ro_weather(id: &str) -> CapabilityGeneration {
        invalidate_instance_capability(id);
        let gen = begin_instance_generation(id);
        let mut get = weather_get_undeclared();
        get.effect = Some(CommandEffect::ReadOnly);
        get.replay_safety = Some(CommandReplaySafety::Safe);
        ExtensionCommandSnapshot::build(id, "2.14.0", None, vec![get], gen.clone())
            .publish_capabilities()
            .unwrap();
        gen
    }

    fn publish_mutating_weather(id: &str) -> CapabilityGeneration {
        invalidate_instance_capability(id);
        let gen = begin_instance_generation(id);
        let mut get = weather_get_undeclared();
        get.effect = Some(CommandEffect::Mutating);
        get.replay_safety = Some(CommandReplaySafety::Unsafe);
        ExtensionCommandSnapshot::build(id, "2.14.0", None, vec![get], gen.clone())
            .publish_capabilities()
            .unwrap();
        gen
    }

    #[test]
    fn nm240_ro_classify_authorizes_and_single_consume() {
        let id = "weather-forecast-v840";
        publish_ro_weather(id);
        let params = json!({"city": "Xiamen"});

        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind.effect, CommandEffect::ReadOnly);
        let op = bind
            .prepared_operation_id
            .expect("RO classify must issue id");
        let prep = claim_prepared_by_id(&op).unwrap();
        assert_eq!(prep.contract.effect, CommandEffect::ReadOnly);
        assert_eq!(prep.operation_id, op);

        let consumed =
            consume_chat_prepared_for_execute(&prep.operation_id, id, "get_weather", &params)
                .unwrap();
        assert_eq!(consumed.operation_id, prep.operation_id);

        let err = consume_chat_prepared_for_execute(&prep.operation_id, id, "get_weather", &params)
            .unwrap_err();
        assert!(
            err.contains("missing ticket") || err.contains("already consumed"),
            "single-consume: {err}"
        );
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm240_stale_after_reload_refuses_consume() {
        let id = "weather-forecast-v841";
        publish_ro_weather(id);
        let params = json!({"city": "Xiamen"});
        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind.effect, CommandEffect::ReadOnly);
        let op = bind.prepared_operation_id.unwrap();
        let prep = claim_prepared_by_id(&op).unwrap();
        let gen = prep.capability_generation.clone();
        let fp = prep.param_fingerprint.clone();

        begin_instance_generation(id);

        let err = consume_authorized_prepared(&op, &gen, &fp).unwrap_err();
        assert!(
            err.contains("stale") || err.contains("missing ticket"),
            "reload must stale Chat token: {err}"
        );
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm240_ro_to_mutating_cannot_skip_gate() {
        let id = "weather-forecast-v842";
        publish_ro_weather(id);
        let params = json!({"city": "Xiamen"});

        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind.effect, CommandEffect::ReadOnly);
        let old_id = bind.prepared_operation_id.unwrap();
        let old = claim_prepared_by_id(&old_id).unwrap();
        assert_eq!(old.contract.effect, CommandEffect::ReadOnly);

        publish_mutating_weather(id);

        let err = consume_chat_prepared_for_execute(&old.operation_id, id, "get_weather", &params)
            .unwrap_err();
        assert!(
            err.contains("stale") || err.contains("missing ticket"),
            "RO→Mutating must not reuse old auth: {err}"
        );

        // Claiming the classify-time id after reload must fail (no re-prepare).
        let err = claim_prepared_by_id(&old_id).unwrap_err();
        assert!(
            err.contains("stale") || err.contains("missing"),
            "execute claim must not mint after reload: {err}"
        );
        assert!(
            find_authorized_prepared(id, "get_weather", &old.param_fingerprint).is_none(),
            "claim must not leave an authorized Mutating ticket"
        );

        let bind2 = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind2.effect, CommandEffect::Mutating);
        let probe = prepare_and_issue_for_chat(id, "get_weather", &params).unwrap();
        assert_eq!(probe.contract.effect, CommandEffect::Mutating);
        let err = consume_authorized_prepared(
            &probe.operation_id,
            &probe.capability_generation,
            &probe.param_fingerprint,
        )
        .unwrap_err();
        assert!(
            err.contains("not Chat-authorized"),
            "Mutating ticket must wait for gate authorize: {err}"
        );
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm240_mutating_to_ro_old_auth_cannot_transfer() {
        let id = "weather-forecast-v843";
        publish_mutating_weather(id);
        let params = json!({"city": "Xiamen"});

        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind.effect, CommandEffect::Mutating);
        let mut_id = bind.prepared_operation_id.unwrap();
        let mut_prep = claim_prepared_by_id(&mut_id).unwrap();
        assert_eq!(mut_prep.contract.effect, CommandEffect::Mutating);

        publish_ro_weather(id);
        let bind_ro = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind_ro.effect, CommandEffect::ReadOnly);
        let ro_id = bind_ro.prepared_operation_id.unwrap();
        let ro_prep = claim_prepared_by_id(&ro_id).unwrap();
        assert_eq!(ro_prep.contract.effect, CommandEffect::ReadOnly);
        assert_ne!(mut_prep.operation_id, ro_prep.operation_id);
        assert_ne!(
            mut_prep.capability_generation,
            ro_prep.capability_generation
        );

        let err = consume_authorized_prepared(
            &mut_prep.operation_id,
            &mut_prep.capability_generation,
            &mut_prep.param_fingerprint,
        )
        .unwrap_err();
        assert!(
            err.contains("stale") || err.contains("missing ticket"),
            "Mutating→RO old auth must not transfer: {err}"
        );

        let err = authorize_prepared_for_chat(
            &mut_prep.operation_id,
            &ro_prep.capability_generation,
            &ro_prep.param_fingerprint,
        )
        .unwrap_err();
        assert!(
            err.contains("unknown") || err.contains("mismatch") || err.contains("missing"),
            "cross-generation authorize must fail: {err}"
        );
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm240_param_change_invalidates_token() {
        let id = "weather-forecast-v844";
        publish_ro_weather(id);
        let params_a = json!({"city": "Xiamen"});
        let params_b = json!({"city": "Beijing"});

        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params_a);
        assert_eq!(bind.effect, CommandEffect::ReadOnly);
        let op = bind.prepared_operation_id.unwrap();
        let prep = claim_prepared_by_id(&op).unwrap();

        let err =
            consume_chat_prepared_for_execute(&prep.operation_id, id, "get_weather", &params_b)
                .unwrap_err();
        assert!(
            err.contains("fingerprint mismatch"),
            "param change must invalidate: {err}"
        );
        // Re-claim same id is already claimed; dispatch check with drifted
        // params must fail closed without minting.
        let err = claim_prepared_for_dispatch(&op, id, "get_weather", &params_b).unwrap_err();
        assert!(
            err.contains("already claimed")
                || err.contains("fingerprint mismatch")
                || err.contains("stale"),
            "param drift claim must fail closed: {err}"
        );
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm240_bind_operation_generation_fingerprint() {
        let id = "weather-forecast-v845";
        publish_ro_weather(id);
        let params = json!({"city": "Xiamen"});
        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind.effect, CommandEffect::ReadOnly);
        let op = bind.prepared_operation_id.unwrap();
        let prep = claim_prepared_by_id(&op).unwrap();

        let err = consume_authorized_prepared(
            &prep.operation_id,
            "gen_not_real",
            &prep.param_fingerprint,
        )
        .unwrap_err();
        assert!(
            err.contains("generation mismatch") || err.contains("stale"),
            "{err}"
        );

        let err = consume_authorized_prepared(
            &prep.operation_id,
            &prep.capability_generation,
            "fp_not_real",
        )
        .unwrap_err();
        assert!(err.contains("fingerprint mismatch"), "{err}");

        consume_authorized_prepared(
            &prep.operation_id,
            &prep.capability_generation,
            &prep.param_fingerprint,
        )
        .unwrap();
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm240_timeout_query_same_op_no_reexec_via_single_consume() {
        let id = "weather-forecast-v846";
        publish_ro_weather(id);
        let params = json!({"city": "Xiamen"});
        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind.effect, CommandEffect::ReadOnly);
        let op = bind.prepared_operation_id.unwrap();
        let prep = claim_prepared_by_id(&op).unwrap();

        consume_authorized_prepared(&op, &prep.capability_generation, &prep.param_fingerprint)
            .unwrap();

        let err =
            consume_authorized_prepared(&op, &prep.capability_generation, &prep.param_fingerprint)
                .unwrap_err();
        assert!(
            err.contains("already consumed") || err.contains("missing ticket"),
            "same op must not re-exec: {err}"
        );
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm240_concurrent_same_params_get_distinct_tickets() {
        // Each classify mints its own id; each dispatch claims **its** id —
        // not "two ids from FIFO prove ownership".
        let id = "weather-forecast-v847";
        publish_ro_weather(id);
        let params = json!({"city": "Xiamen"});
        let a_bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        let b_bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(a_bind.effect, CommandEffect::ReadOnly);
        assert_eq!(b_bind.effect, CommandEffect::ReadOnly);
        let a_id = a_bind.prepared_operation_id.unwrap();
        let b_id = b_bind.prepared_operation_id.unwrap();
        assert_ne!(a_id, b_id, "concurrent same-params must mint distinct ids");

        let a = claim_prepared_by_id(&a_id).unwrap();
        let b = claim_prepared_by_id(&b_id).unwrap();
        assert_eq!(a.operation_id, a_id);
        assert_eq!(b.operation_id, b_id);

        // Double-claim of A's id is rejected under the same lock.
        let err = claim_prepared_by_id(&a_id).unwrap_err();
        assert!(
            err.contains("already claimed"),
            "second claim of same id must fail: {err}"
        );
        invalidate_instance_capability(id);
    }

    /// Production-path regression: RO classify → reload to Mutating → claim
    /// by the **old** id must fail stale and must **not** mint+authorize.
    #[test]
    fn nm240_claim_after_ro_reload_to_mutating_does_not_reprepare() {
        let id = "weather-forecast-v848";
        publish_ro_weather(id);
        let params = json!({"city": "Xiamen"});
        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind.effect, CommandEffect::ReadOnly);
        let old_id = bind.prepared_operation_id.unwrap();

        publish_mutating_weather(id);

        let err = claim_prepared_by_id(&old_id).unwrap_err();
        assert!(
            err.contains("stale") || err.contains("missing") || err.contains("reload"),
            "RO→reload→Mutating claim must be stale, got: {err}"
        );
        let probe = prepare_and_issue_for_chat(id, "get_weather", &params).unwrap();
        assert_eq!(probe.contract.effect, CommandEffect::Mutating);
        assert!(
            find_authorized_prepared(id, "get_weather", &probe.param_fingerprint).is_none(),
            "execute path must not authorize a fresh Mutating prepare"
        );
        let bind2 = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind2.effect, CommandEffect::Mutating);
        invalidate_instance_capability(id);
    }

    /// Interleave: A RO classify → reload → B Mutating classify → A executes
    /// with A's id → stale. B's ticket must not be claimed/authorized by A;
    /// no cross-dispatch FIFO handoff.
    #[test]
    fn nm240_interleave_a_ro_reload_b_mutating_a_cannot_claim_b() {
        let id = "weather-forecast-v849";
        publish_ro_weather(id);
        let params = json!({"city": "Xiamen"});

        let a_bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(a_bind.effect, CommandEffect::ReadOnly);
        let a_id = a_bind
            .prepared_operation_id
            .expect("A must carry its ticket id");

        publish_mutating_weather(id);

        let b_bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(b_bind.effect, CommandEffect::Mutating);
        let b_id = b_bind
            .prepared_operation_id
            .expect("B must issue a new ticket");
        assert_ne!(a_id, b_id);

        // A resumes with A's classify-time id — stale, not B's ticket.
        let err = claim_prepared_by_id(&a_id).unwrap_err();
        assert!(
            err.contains("stale") || err.contains("missing"),
            "A must not execute after reload via own id: {err}"
        );
        // A also must not be able to "discover" B by params — that API is gone;
        // claiming B requires B's id, which A does not carry.
        let b_peek = peek_prepared_invoke(&b_id).expect("B ticket still present");
        assert_eq!(b_peek.contract.effect, CommandEffect::Mutating);
        assert_eq!(b_peek.operation_id, b_id);
        // B remains unclaimed / unauthorized until B's own dispatch claims it.
        let err = consume_authorized_prepared(
            &b_id,
            &b_peek.capability_generation,
            &b_peek.param_fingerprint,
        )
        .unwrap_err();
        assert!(
            err.contains("not Chat-authorized"),
            "A's failed claim must not authorize B: {err}"
        );
        // Double-check: claiming B with B's id still works for B's dispatch.
        let b_claimed = claim_prepared_by_id(&b_id).unwrap();
        assert_eq!(b_claimed.operation_id, b_id);
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm240_claim_prepared_by_id_rejects_already_claimed_same_lock() {
        let id = "weather-forecast-v850";
        publish_ro_weather(id);
        let params = json!({"city": "Xiamen"});
        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        let op = bind.prepared_operation_id.unwrap();
        claim_prepared_by_id(&op).unwrap();
        let err = claim_prepared_by_id(&op).unwrap_err();
        assert!(
            err.contains("already claimed"),
            "must reject already-claimed under same lock: {err}"
        );
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm247_params_example_uses_options_as_hints_not_enum() {
        let cmd = weather_get_undeclared();
        let schema = build_input_schema(&cmd.parameters);
        assert!(
            schema["properties"]["city"].get("enum").is_none(),
            "non-Enum options must not become schema enum"
        );
        // Valid but not in options — free-text + hints still accepts.
        assert!(validate_and_normalize(&schema, &json!({"city": "Xiamen"})).is_ok());
        let ex = validated_params_example_for_command(&cmd).expect("hint sample");
        assert_eq!(ex.get("city").and_then(|c| c.as_str()), Some("Beijing"));
        assert!(validate_and_normalize(&schema, &ex).is_ok());
    }

    #[test]
    fn nm247_enum_param_stays_closed() {
        let cmd = ExtensionCommand {
            name: "pick".into(),
            display_name: "Pick".into(),
            description: "Closed enum".into(),
            parameters: vec![ParameterDefinition {
                name: "mode".into(),
                display_name: "Mode".into(),
                description: "Mode".into(),
                param_type: MetricDataType::Enum {
                    options: vec!["safe".into(), "fast".into()],
                },
                required: true,
                default_value: None,
                min: None,
                max: None,
                options: vec![],
            }],
            ..Default::default()
        };
        let schema = build_input_schema(&cmd.parameters);
        assert_eq!(
            schema["properties"]["mode"]["enum"],
            json!(["safe", "fast"])
        );
        let err = validate_and_normalize(&schema, &json!({"mode": "other"})).unwrap_err();
        assert!(
            err.contains("enum") || err.contains("one of") || err.contains("allowed"),
            "closed enum must reject outside value: {err}"
        );
        let ex = validated_params_example_for_command(&cmd).unwrap();
        assert_eq!(ex.get("mode").and_then(|v| v.as_str()), Some("safe"));
    }

    #[test]
    fn nm247_params_example_prefers_author_samples() {
        let mut cmd = weather_get_undeclared();
        cmd.parameters[0].options = vec!["Beijing".into(), "Shanghai".into(), "New York".into()];
        cmd.samples = vec![json!({"city": "Shanghai"}), json!({"city": "Beijing"})];
        let ex = validated_params_example_for_command(&cmd).unwrap();
        assert_eq!(ex.get("city").and_then(|c| c.as_str()), Some("Shanghai"));
    }

    #[test]
    fn nm247_ai_extension_catalog_lists_published_pair() {
        for id in ["weather-nm247-cat", "ocr-nm247-cat", "yolo-nm247-cat"] {
            invalidate_instance_capability(id);
            let gen = begin_instance_generation(id);
            ExtensionCommandSnapshot::build(id, "1.0.0", None, vec![weather_get_undeclared()], gen)
                .publish_capabilities()
                .unwrap();
            set_instance_ai_visibility(id, true, vec![]);
        }
        let catalog = format_ai_extension_catalog(12, 2000);
        assert!(catalog.contains("weather-nm247-cat"), "{catalog}");
        assert!(catalog.contains("ocr-nm247-cat"), "{catalog}");
        assert!(catalog.contains("yolo-nm247-cat"), "{catalog}");
        assert!(
            catalog.contains("--search"),
            "must teach search discovery: {catalog}"
        );
        for id in ["weather-nm247-cat", "ocr-nm247-cat", "yolo-nm247-cat"] {
            invalidate_instance_capability(id);
        }
    }

    #[test]
    fn nm252_search_ranks_weather_in_top3_among_many() {
        let weather = "weather-nm252-search";
        invalidate_instance_capability(weather);
        let gen = begin_instance_generation(weather);
        ExtensionCommandSnapshot::build(
            weather,
            "1.0.0",
            None,
            vec![weather_get_undeclared()],
            gen,
        )
        .publish_capabilities()
        .unwrap();
        set_instance_ai_visibility(weather, true, vec![]);

        // Many unrelated AI-visible commands.
        for i in 0..40 {
            let id = format!("noise-nm252-{i}");
            invalidate_instance_capability(&id);
            let gen = begin_instance_generation(&id);
            let mut cmd = weather_get_undeclared();
            cmd.name = format!("do_thing_{i}");
            cmd.description = format!("Unrelated operation number {i}");
            cmd.parameters.clear();
            ExtensionCommandSnapshot::build(&id, "1.0.0", None, vec![cmd], gen)
                .publish_capabilities()
                .unwrap();
            set_instance_ai_visibility(&id, true, vec![]);
        }

        let few = search_ai_extension_commands(
            "weather in Xiamen",
            None,
            AI_COMMAND_SEARCH_MAX_HITS,
            AI_COMMAND_SEARCH_MAX_CHARS,
        );
        assert!(
            few.hits
                .iter()
                .any(|h| h.extension_id == weather && h.command == "get_weather"),
            "weather must rank in top hits with few+noise: {few:?}"
        );
        assert!(few.hits.len() <= AI_COMMAND_SEARCH_MAX_HITS, "{few:?}");
        assert!(
            few.hits
                .iter()
                .any(|h| h.required_params.iter().any(|p| p.name == "city")),
            "required params must surface: {few:?}"
        );
        let approx: usize = few.hits.iter().map(estimate_hit_chars).sum();
        assert!(
            approx <= AI_COMMAND_SEARCH_MAX_CHARS,
            "budget ~500tok: {approx} chars for {few:?}"
        );

        // Disable / unload reflections.
        set_instance_ai_visibility(weather, false, vec![]);
        let off = search_ai_extension_commands("weather", None, 3, 2000);
        assert!(
            !off.hits.iter().any(|h| h.extension_id == weather),
            "ai_enabled=false must hide: {off:?}"
        );

        set_instance_ai_visibility(weather, true, vec!["get_weather".into()]);
        let cmd_off = search_ai_extension_commands("weather", None, 3, 2000);
        assert!(
            !cmd_off.hits.iter().any(|h| h.extension_id == weather),
            "disabled_commands must hide: {cmd_off:?}"
        );

        set_instance_ai_visibility(weather, true, vec![]);
        invalidate_instance_capability(weather);
        let unloaded = search_ai_extension_commands("weather", None, 3, 2000);
        assert!(
            !unloaded.hits.iter().any(|h| h.extension_id == weather),
            "unload must drop from next search: {unloaded:?}"
        );

        let no_match =
            search_ai_extension_commands("quantum teleportation protocol zzz", None, 3, 2000);
        assert!(
            no_match.hits.is_empty(),
            "low relevance must allow zero hits: {no_match:?}"
        );

        // Scoped @-style search still works after re-publish.
        let gen2 = begin_instance_generation(weather);
        ExtensionCommandSnapshot::build(
            weather,
            "1.0.0",
            None,
            vec![weather_get_undeclared()],
            gen2,
        )
        .publish_capabilities()
        .unwrap();
        set_instance_ai_visibility(weather, true, vec![]);
        let scoped = search_ai_extension_commands("weather", Some(weather), 3, 2000);
        assert_eq!(scoped.hits.len(), 1, "{scoped:?}");
        assert_eq!(scoped.hits[0].extension_id, weather);

        let rules = format_extension_discovery_rules();
        assert!(rules.contains("--search"), "{rules}");
        assert!(rules.contains("reuse"), "{rules}");

        invalidate_instance_capability(weather);
        for i in 0..40 {
            invalidate_instance_capability(&format!("noise-nm252-{i}"));
        }
    }

    #[test]
    fn nm252_search_honors_ai_exposed_false() {
        let id = "weather-nm252-exposed";
        invalidate_instance_capability(id);
        let gen = begin_instance_generation(id);
        let mut hidden = weather_get_undeclared();
        hidden.ai_exposed = false;
        ExtensionCommandSnapshot::build(id, "1.0.0", None, vec![hidden], gen)
            .publish_capabilities()
            .unwrap();
        set_instance_ai_visibility(id, true, vec![]);
        let r = search_ai_extension_commands("weather", None, 3, 2000);
        assert!(
            !r.hits.iter().any(|h| h.extension_id == id),
            "ai_exposed=false omitted: {r:?}"
        );
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm247_catalog_honors_ai_enabled_and_disabled_commands() {
        let id = "weather-nm247-ai-vis";
        invalidate_instance_capability(id);
        let gen = begin_instance_generation(id);
        let mut get = weather_get_undeclared();
        let mut hidden = weather_get_undeclared();
        hidden.name = "hidden_cmd".into();
        hidden.ai_exposed = false;
        let mut other = weather_get_undeclared();
        other.name = "forecast".into();
        ExtensionCommandSnapshot::build(id, "1.0.0", None, vec![get.clone(), hidden, other], gen)
            .publish_capabilities()
            .unwrap();

        set_instance_ai_visibility(id, true, vec![]);
        let on = format_ai_extension_catalog(12, 2000);
        assert!(on.contains(id), "AI on must list: {on}");

        set_instance_ai_visibility(id, false, vec![]);
        let off = format_ai_extension_catalog(12, 2000);
        assert!(
            !off.contains(id),
            "ai_enabled=false must hide whole extension: {off}"
        );

        set_instance_ai_visibility(id, true, vec!["get_weather".into(), "forecast".into()]);
        let one_off = format_ai_extension_catalog(12, 2000);
        assert!(
            !one_off.contains(id),
            "all AI-visible cmds disabled → omit extension: {one_off}"
        );

        set_instance_ai_visibility(id, true, vec!["get_weather".into()]);
        let partial = format_ai_extension_catalog(12, 2000);
        assert!(
            partial.contains(id),
            "re-enable via remaining cmd must list: {partial}"
        );

        // ai_exposed=false alone never lists that command; with only that left, omit.
        get.ai_exposed = false;
        let gen2 = begin_instance_generation(id);
        ExtensionCommandSnapshot::build(id, "1.0.0", None, vec![get], gen2)
            .publish_capabilities()
            .unwrap();
        set_instance_ai_visibility(id, true, vec![]);
        let author_hide = format_ai_extension_catalog(12, 2000);
        assert!(
            !author_hide.contains(id),
            "ai_exposed=false only → omit: {author_hide}"
        );

        invalidate_instance_capability(id);
    }

    #[test]
    fn nm247_catalog_skips_without_ai_visibility_policy() {
        let id = "weather-nm247-no-policy";
        invalidate_instance_capability(id);
        let gen = begin_instance_generation(id);
        ExtensionCommandSnapshot::build(id, "1.0.0", None, vec![weather_get_undeclared()], gen)
            .publish_capabilities()
            .unwrap();
        // Register / prepareable published, but AI policy not synced yet.
        let before = format_ai_extension_catalog(12, 2000);
        assert!(
            !before.contains(id),
            "register before policy publish must not list: {before}"
        );
        set_instance_ai_visibility(id, true, vec![]);
        let after = format_ai_extension_catalog(12, 2000);
        assert!(
            after.contains(id),
            "after policy publish must list: {after}"
        );
        invalidate_instance_capability(id);
    }

    #[test]
    fn nm247_catalog_byte_budget_keeps_complete_entries() {
        for id in ["a-nm247-budget", "b-nm247-budget", "c-nm247-budget"] {
            invalidate_instance_capability(id);
            let gen = begin_instance_generation(id);
            ExtensionCommandSnapshot::build(id, "1.0.0", None, vec![weather_get_undeclared()], gen)
                .publish_capabilities()
                .unwrap();
            set_instance_ai_visibility(id, true, vec![]);
        }
        // Tiny budget: only complete lines fit; never a mid-line cut of an id.
        let catalog = format_ai_extension_catalog(12, 120);
        if !catalog.is_empty() {
            for line in catalog.lines() {
                if let Some(rest) = line.strip_prefix("- ") {
                    let id = rest.split(" — ").next().unwrap_or("");
                    assert!(
                        !id.is_empty() && !id.ends_with('…'),
                        "must not emit truncated entry id: {line}"
                    );
                }
            }
        }
        for id in ["a-nm247-budget", "b-nm247-budget", "c-nm247-budget"] {
            invalidate_instance_capability(id);
        }
    }
}
