//! Lightweight domain command contract registry for core NeoMind business domains.
//!
//! Covers key mutating/execution commands across the four core business domains
//! (**Device**, **Rule**, **Dashboard**, **Agent**) without imposing heavy
//! abstractions on simple CRUD commands (e.g. `settings timezone`).
//!
//! Acts as the single source of truth shared across:
//! - `neomind-cli-ops` (`json_arg::PRIMARY_JSON_SPECS`, `capability`, discovery hints)
//! - `neomind-agent` (`ShellTool` structured `params` validation, `recovery_hint`,
//!   `derive_tool_capability_resolved`, `intent` write fingerprinting)
//! - anti-drift unit tests cross-checking live clap `Args::command()` definitions.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Expected top-level JSON container kind for a command's primary JSON flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedJsonKind {
    Object,
    Array,
}

/// Primary JSON flag specification for a `neomind <domain> <action>` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrimaryJsonSpec {
    pub flag: &'static str,
    pub kind: ExpectedJsonKind,
}

/// Side-effect classification for a domain command contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainCommandSideEffect {
    ReadOnly,
    Mutating,
}

/// Concise summary of a key parameter or flag on a domain command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainParamSummary {
    /// Positional placeholder (e.g. `<id>`) or CLI long flag (e.g. `--name`).
    pub name: &'static str,
    /// Whether this argument is required on its canonical invocation path.
    pub required: bool,
    /// One-line description for discovery and validation hints.
    pub summary: &'static str,
}

/// Canonical quote-free structured shell example (`{command, params?}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainShellExample {
    /// Base `neomind <domain> <action> ...` command line (without inline JSON when `params_json` is set).
    pub command: &'static str,
    /// Optional structured JSON payload for the shell tool's `params` field.
    pub params_json: Option<&'static str>,
}

impl DomainShellExample {
    /// Render as the canonical JSON object expected by the agent `shell` tool.
    pub fn to_json_value(&self) -> Value {
        let parsed = self
            .params_json
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
        build_structured_shell_example(self.command, parsed.as_ref())
    }
}

/// Build a canonical structured `shell_example` JSON object (`{command}` or
/// `{command, params}`).
///
/// Shared by core domain contracts and dynamic discovery projections (such as
/// `device get` and `extension commands --search`) so every model-facing
/// surface teaches the exact same quote-free invocation shape.
pub fn build_structured_shell_example(command: &str, params: Option<&Value>) -> Value {
    match params {
        Some(p) => json!({
            "command": command,
            "params": p,
        }),
        None => json!({
            "command": command,
        }),
    }
}

/// Lightweight contract for a core business domain command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainCommandContract {
    pub domain: &'static str,
    pub action: &'static str,
    pub side_effect: DomainCommandSideEffect,
    pub primary_json: Option<PrimaryJsonSpec>,
    pub key_params: &'static [DomainParamSummary],
    pub shell_example: DomainShellExample,
    pub guidance: &'static str,
}

impl DomainCommandContract {
    /// Validate a structured JSON value intended for this command's primary JSON flag.
    pub fn validate_primary_json(&self, value: &Value) -> Result<(), String> {
        let Some(spec) = self.primary_json else {
            return Ok(());
        };
        match spec.kind {
            ExpectedJsonKind::Object => {
                if !value.is_object() {
                    return Err(format!(
                        "shell params for `{}` on `neomind {} {}` must be a JSON object",
                        spec.flag, self.domain, self.action
                    ));
                }
            }
            ExpectedJsonKind::Array => {
                if !value.is_array() {
                    return Err(format!(
                        "shell params for `{}` on `neomind {} {}` must be a JSON array",
                        spec.flag, self.domain, self.action
                    ));
                }
            }
        }

        // Domain-specific lightweight structural guardrails (never replaces full
        // backend validation; catches high-frequency model shape mistakes early).
        match (self.domain, self.action) {
            ("device", "control") => {
                if let Some(obj) = value.as_object() {
                    if obj.contains_key("cmd")
                        && obj.get("params").is_some_and(|v| v.is_object())
                        && obj.len() <= 3
                    {
                        return Err(
                            "do not wrap device control params in a protocol envelope (`{\"cmd\":...,\"params\":{...}}`); pass only the command's parameter object from `device get <ID>`"
                                .to_string(),
                        );
                    }
                }
            }
            ("dashboard", "add-components") => {
                if let Some(arr) = value.as_array() {
                    if arr.is_empty() {
                        return Err(
                            "dashboard add-components requires a non-empty JSON array of component objects"
                                .to_string(),
                        );
                    }
                    if arr.iter().any(|item| !item.is_object()) {
                        return Err(
                            "each item in dashboard add-components `params` must be a JSON object"
                                .to_string(),
                        );
                    }
                }
            }
            ("dashboard", "remove-components") => {
                if let Some(arr) = value.as_array() {
                    if arr.is_empty() {
                        return Err(
                            "dashboard remove-components requires a non-empty JSON array of component ID strings"
                                .to_string(),
                        );
                    }
                    if arr
                        .iter()
                        .any(|item| item.as_str().map(str::trim).unwrap_or("").is_empty())
                    {
                        return Err(
                            "each item in dashboard remove-components `params` must be a non-empty component ID string"
                                .to_string(),
                        );
                    }
                }
            }
            ("dashboard", "update-component") => {
                if let Some(obj) = value.as_object() {
                    if obj.is_empty() {
                        return Err(
                            "dashboard update-component `--set` / `params` must be a non-empty JSON object of fields to patch"
                                .to_string(),
                        );
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Render the canonical structured `shell_example` JSON value.
    pub fn shell_example_value(&self) -> Value {
        self.shell_example.to_json_value()
    }

    /// Compact recovery / discovery hint derived directly from this contract.
    pub fn format_hint(&self) -> String {
        let example = self.shell_example_value();
        let req: Vec<&str> = self
            .key_params
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name)
            .collect();
        if req.is_empty() {
            format!("{} Example: {}", self.guidance, example)
        } else {
            format!(
                "{} Required: {}. Example: {}",
                self.guidance,
                req.join(", "),
                example
            )
        }
    }
}

/// Canonical registry of core business domain command contracts across
/// Device, Rule, Dashboard, and Agent.
pub const CORE_DOMAIN_CONTRACTS: &[DomainCommandContract] = &[
    // === Device ===
    DomainCommandContract {
        domain: "device",
        action: "control",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--params",
            kind: ExpectedJsonKind::Object,
        }),
        key_params: &[
            DomainParamSummary {
                name: "<id>",
                required: true,
                summary: "Target device ID (positional; discover via `device list`)",
            },
            DomainParamSummary {
                name: "<command>",
                required: true,
                summary: "Command name defined by the device type template (`device get <ID>`)",
            },
            DomainParamSummary {
                name: "--params",
                required: false,
                summary: "Command parameter object (prefer shell structured `params: {...}`)",
            },
            DomainParamSummary {
                name: "--param",
                required: false,
                summary: "Repeatable `key=value` fallback for simple scalar parameters",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind device control device-001 toggle",
            params_json: Some(r#"{"state":true}"#),
        },
        guidance: "Pass the command's parameter object from `device get <ID>` via shell `params` (do not wrap in a protocol envelope).",
    },
    // === Rule ===
    DomainCommandContract {
        domain: "rule",
        action: "create",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--body",
            kind: ExpectedJsonKind::Object,
        }),
        key_params: &[
            DomainParamSummary {
                name: "--body",
                required: false,
                summary: "Full rule JSON object (`name`, `condition`, `actions`; prefer shell `params`)",
            },
            DomainParamSummary {
                name: "--name",
                required: false,
                summary: "Rule name (flag fast-path, mutually exclusive with `--body`)",
            },
            DomainParamSummary {
                name: "--trigger-device",
                required: false,
                summary: "Device ID to watch (with `--metric`, `--operator`, `--threshold`, `--notify`)",
            },
            DomainParamSummary {
                name: "--operator",
                required: false,
                summary: "greater_than | less_than | greater_equal | less_equal | equal | not_equal",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind rule create",
            params_json: Some(
                r#"{"name":"High Temp","condition":{"condition_type":"comparison","source":"device:sensor-001:temperature","operator":"greater_than","threshold":30},"actions":[{"type":"notify","message":"Too hot","severity":"warning"}]}"#,
            ),
        },
        guidance: "Discover real device IDs and metric fields via `device list` / `device get <ID>` before creating a rule; pass full rule JSON via shell `params` (maps to `--body`) or use flag fast-path.",
    },
    DomainCommandContract {
        domain: "rule",
        action: "update",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--body",
            kind: ExpectedJsonKind::Object,
        }),
        key_params: &[
            DomainParamSummary {
                name: "<id>",
                required: true,
                summary: "Rule ID (positional or `--id`)",
            },
            DomainParamSummary {
                name: "--body",
                required: false,
                summary: "Partial or full rule JSON object (prefer shell `params`)",
            },
            DomainParamSummary {
                name: "--name",
                required: false,
                summary: "Updated rule name (flag fast-path)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind rule update rule-001",
            params_json: Some(r#"{"name":"Updated Rule"}"#),
        },
        guidance: "Pass rule updates via shell `params` (maps to `--body`) or use flag fast-path (`--threshold`, `--notify`, `--name`).",
    },
    DomainCommandContract {
        domain: "rule",
        action: "test",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--input",
            kind: ExpectedJsonKind::Object,
        }),
        key_params: &[
            DomainParamSummary {
                name: "<id>",
                required: true,
                summary: "Rule ID (positional)",
            },
            DomainParamSummary {
                name: "--input",
                required: true,
                summary: "Sample metric values JSON object (prefer shell `params`)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind rule test rule-001",
            params_json: Some(r#"{"temperature":32}"#),
        },
        guidance: "Evaluate a rule against sample metric input passed via shell `params` (maps to `--input`).",
    },
    // === Dashboard ===
    DomainCommandContract {
        domain: "dashboard",
        action: "create",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--components",
            kind: ExpectedJsonKind::Array,
        }),
        key_params: &[
            DomainParamSummary {
                name: "--name",
                required: true,
                summary: "Dashboard name",
            },
            DomainParamSummary {
                name: "--description",
                required: false,
                summary: "Optional dashboard description",
            },
            DomainParamSummary {
                name: "--components",
                required: false,
                summary: "Optional initial components JSON array (prefer shell `params: [...]`)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind dashboard create --name \"Operations\"",
            params_json: Some(
                r#"[{"id":"c1","type":"value-card","title":"Temperature","position":{"x":0,"y":0,"w":4,"h":2}}]"#,
            ),
        },
        guidance: "Provide `--name` and optionally pass initial widgets as a JSON array via shell `params` (maps to `--components`).",
    },
    DomainCommandContract {
        domain: "dashboard",
        action: "update",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--components",
            kind: ExpectedJsonKind::Array,
        }),
        key_params: &[
            DomainParamSummary {
                name: "<id>",
                required: true,
                summary: "Dashboard ID (positional)",
            },
            DomainParamSummary {
                name: "--name",
                required: false,
                summary: "Updated dashboard name",
            },
            DomainParamSummary {
                name: "--components",
                required: false,
                summary: "Full replacement components JSON array (requires `--replace-all`)",
            },
            DomainParamSummary {
                name: "--replace-all",
                required: false,
                summary: "Required confirmation flag when replacing all components via `--components`",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind dashboard update dash-001 --replace-all",
            params_json: Some(
                r#"[{"id":"c1","type":"value-card","title":"Temperature","position":{"x":0,"y":0,"w":4,"h":2}}]"#,
            ),
        },
        guidance: "`dashboard update --components` replaces ALL widgets and requires `--replace-all`; prefer `update-component` for single-widget tweaks or `add-components` to append.",
    },
    DomainCommandContract {
        domain: "dashboard",
        action: "add-components",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--components",
            kind: ExpectedJsonKind::Array,
        }),
        key_params: &[
            DomainParamSummary {
                name: "<id>",
                required: true,
                summary: "Dashboard ID (positional)",
            },
            DomainParamSummary {
                name: "--components",
                required: true,
                summary: "Non-empty JSON array of components to append (prefer shell `params: [...]`)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind dashboard add-components dash-001",
            params_json: Some(
                r#"[{"id":"c2","type":"value-card","title":"Humidity","position":{"x":0,"y":2,"w":4,"h":2}}]"#,
            ),
        },
        guidance: "Append widgets via shell `params: […]` (maps to `--components`); add only 1–2 components per call; never invent CLI `--params`; bind live values with `data_source` (not ephemeral `data`/`label`).",
    },
    DomainCommandContract {
        domain: "dashboard",
        action: "update-component",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--set",
            kind: ExpectedJsonKind::Object,
        }),
        key_params: &[
            DomainParamSummary {
                name: "<id>",
                required: true,
                summary: "Dashboard ID (positional)",
            },
            DomainParamSummary {
                name: "--component-id",
                required: true,
                summary: "Component ID from `dashboard get <ID>`",
            },
            DomainParamSummary {
                name: "--set",
                required: true,
                summary: "Partial JSON object deep-merged into the component (prefer shell `params: {...}`)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind dashboard update-component dash-001 --component-id c1",
            params_json: Some(r#"{"position":{"w":2,"h":2}}"#),
        },
        guidance: "Deep-merge only changed fields into one widget via shell `params` (maps to `--set`); keep existing `x`/`y` when resizing in place.",
    },
    DomainCommandContract {
        domain: "dashboard",
        action: "remove-components",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--ids",
            kind: ExpectedJsonKind::Array,
        }),
        key_params: &[
            DomainParamSummary {
                name: "<id>",
                required: true,
                summary: "Dashboard ID (positional)",
            },
            DomainParamSummary {
                name: "--ids",
                required: true,
                summary: "Non-empty JSON array of component ID strings (prefer shell `params: [...]`)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind dashboard remove-components dash-001",
            params_json: Some(r#"["c1","c2"]"#),
        },
        guidance: "Remove specific widgets by passing a JSON array of component IDs in shell `params` (maps to `--ids`).",
    },
    // === Agent ===
    DomainCommandContract {
        domain: "agent",
        action: "create",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[
            DomainParamSummary {
                name: "--name",
                required: true,
                summary: "Agent name (1-100 chars)",
            },
            DomainParamSummary {
                name: "--prompt",
                required: true,
                summary: "Task instruction for the agent",
            },
            DomainParamSummary {
                name: "--schedule-type",
                required: false,
                summary: "interval | cron | event | manual (or use `--every`)",
            },
            DomainParamSummary {
                name: "--execution-mode",
                required: false,
                summary: "focused | free | structured",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind agent create --name \"Sensor Monitor\" --prompt \"Check sensor anomalies\" --schedule-type manual",
            params_json: None,
        },
        guidance: "Provide `--name`, `--prompt`, and schedule (`--schedule-type` + `--schedule-config` or `--every`); discover real device IDs before binding resources.",
    },
    DomainCommandContract {
        domain: "agent",
        action: "update",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[
            DomainParamSummary {
                name: "<id>",
                required: true,
                summary: "Agent ID (positional)",
            },
            DomainParamSummary {
                name: "--name",
                required: false,
                summary: "Updated agent name",
            },
            DomainParamSummary {
                name: "--prompt",
                required: false,
                summary: "Updated task instruction",
            },
            DomainParamSummary {
                name: "--schedule-type",
                required: false,
                summary: "Updated schedule type (interval | cron | event | manual)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind agent update agent-001 --prompt \"Check temperature sensors\"",
            params_json: None,
        },
        guidance: "Pass the agent ID positionally and supply only the flags you want to change (inspect current config first with `agent get <ID>`).",
    },
    DomainCommandContract {
        domain: "agent",
        action: "invoke",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[
            DomainParamSummary {
                name: "<id>",
                required: true,
                summary: "Agent ID (positional)",
            },
            DomainParamSummary {
                name: "[input]",
                required: false,
                summary: "Optional one-time input prompt for this run",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind agent invoke agent-001",
            params_json: None,
        },
        guidance: "Invoke an agent immediately by positional `<ID>` (with optional input string); if it returns `Executing`, check `agent latest-execution <ID>`.",
    },
];

/// Look up a core [`DomainCommandContract`] by `(domain, action)`.
pub fn find_domain_command_contract(
    domain: &str,
    action: &str,
) -> Option<&'static DomainCommandContract> {
    CORE_DOMAIN_CONTRACTS
        .iter()
        .find(|c| c.domain == domain && c.action == action)
}

/// Look up a core [`DomainCommandContract`] for a tokenized `neomind <domain> <action> ...` argv.
pub fn find_domain_command_contract_for_argv(
    argv: &[String],
) -> Option<&'static DomainCommandContract> {
    if argv.len() < 3 || argv[0] != "neomind" {
        return None;
    }
    find_domain_command_contract(argv[1].as_str(), argv[2].as_str())
}

/// Return all registered core domain command contracts.
pub fn core_domain_contracts() -> &'static [DomainCommandContract] {
    CORE_DOMAIN_CONTRACTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_contracts_cover_four_business_domains() {
        let required = [
            ("device", "control"),
            ("rule", "create"),
            ("rule", "update"),
            ("rule", "test"),
            ("dashboard", "create"),
            ("dashboard", "update"),
            ("dashboard", "add-components"),
            ("dashboard", "update-component"),
            ("dashboard", "remove-components"),
            ("agent", "create"),
            ("agent", "update"),
            ("agent", "invoke"),
        ];
        for (domain, action) in required {
            let c = find_domain_command_contract(domain, action)
                .unwrap_or_else(|| panic!("missing contract for `{domain} {action}`"));
            assert_eq!(c.side_effect, DomainCommandSideEffect::Mutating);
            let ex = c.shell_example_value();
            assert!(ex["command"].as_str().is_some());
            if let Some(params) = ex.get("params") {
                c.validate_primary_json(params).unwrap_or_else(|e| {
                    panic!("shell_example for `{domain} {action}` failed validation: {e}")
                });
            }
        }
    }

    #[test]
    fn device_control_rejects_protocol_envelope_and_non_object() {
        let c = find_domain_command_contract("device", "control").unwrap();
        assert!(c.validate_primary_json(&json!({"text": "hello"})).is_ok());
        assert!(c.validate_primary_json(&json!(["bad"])).is_err());
        let env_err = c
            .validate_primary_json(&json!({"cmd": "render_text", "params": {"text": "hi"}}))
            .unwrap_err();
        assert!(env_err.contains("protocol envelope"));
    }

    #[test]
    fn dashboard_array_and_object_contracts_validate_shapes() {
        let add = find_domain_command_contract("dashboard", "add-components").unwrap();
        assert!(add.validate_primary_json(&json!([])).is_err());
        assert!(add.validate_primary_json(&json!({})).is_err());
        assert!(add
            .validate_primary_json(&json!(["not-an-object"]))
            .is_err());
        assert!(add
            .validate_primary_json(&json!([{"id":"c1","type":"value-card"}]))
            .is_ok());

        let rem = find_domain_command_contract("dashboard", "remove-components").unwrap();
        assert!(rem.validate_primary_json(&json!([])).is_err());
        assert!(rem.validate_primary_json(&json!([123])).is_err());
        assert!(rem.validate_primary_json(&json!(["c1"])).is_ok());

        let upd = find_domain_command_contract("dashboard", "update-component").unwrap();
        assert!(upd.validate_primary_json(&json!({})).is_err());
        assert!(upd.validate_primary_json(&json!(["c1"])).is_err());
        assert!(upd
            .validate_primary_json(&json!({"position":{"w":2,"h":2}}))
            .is_ok());
    }
}
