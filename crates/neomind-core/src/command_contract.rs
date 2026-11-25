//! Lightweight domain command contract registry for core NeoMind business domains.
//!
//! Covers key mutating/execution (and a few high-frequency read) commands across
//! ten business domains: **Device**, **Rule**, **Dashboard**, **Agent**,
//! **Widget**, **Extension**, **Settings**, **Connector**, **Message**,
//! **Transform**. Expanded from the original four (Device/Rule/Dashboard/Agent)
//! after NM-322 overnight rejected_args hotspots (v1.3.2 T1.2).
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
/// Device, Rule, Dashboard, Agent, Widget, Extension, Settings, Connector,
/// Message, and Transform.
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
            DomainParamSummary {
                name: "--no-tools",
                required: false,
                summary: "Text-only agent (disables all tools; conflicts with `--allowed-tools` and judgment notify)",
            },
            DomainParamSummary {
                name: "--allowed-tools",
                required: false,
                summary: "Comma-separated allowlist (e.g. `shell`); `all` restores every tool",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind agent create --name \"Sensor Monitor\" --prompt \"Check sensor anomalies\" --every 10m --execution-mode free",
            params_json: None,
        },
        guidance: "Required `--name` + `--prompt` + schedule (`--every 10m` or `--schedule-type interval --schedule-config \"600\"` seconds). Default execution-mode is focused and needs `--device-ids`/`--resources`/`--metrics`/`--commands`; for unbound create→invoke→delete use `--execution-mode free`. Use `--no-tools` for text-only or `--allowed-tools shell` for judgment alerts (never both; never `--no-tools` with `--notify on=judgment`). Prefer `--every` over inventing JSON schedule-config objects.",
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
            DomainParamSummary {
                name: "--event-filter",
                required: false,
                summary: "Event filter JSON; merges into existing schedule when type unchanged",
            },
            DomainParamSummary {
                name: "--timezone",
                required: false,
                summary: "Cron timezone (e.g. Asia/Shanghai); merges into existing schedule",
            },
            DomainParamSummary {
                name: "--no-tools",
                required: false,
                summary: "Disable all tools (conflicts with `--allowed-tools` and judgment notify)",
            },
            DomainParamSummary {
                name: "--allowed-tools",
                required: false,
                summary: "Comma-separated allowlist; `all` restores every tool after `--no-tools`",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind agent update agent-001 --prompt \"Check temperature sensors\"",
            params_json: None,
        },
        guidance: "Pass the agent ID positionally and supply only the flags you want to change (inspect current config first with `agent get <ID>`). `--event-filter` / `--timezone` merge into the existing schedule when `--schedule-type` is omitted. Tool gating: `--no-tools` or `--allowed-tools` (use `all` to re-enable).",
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
            command: "neomind agent invoke agent-001 \"Check sensors now\"",
            params_json: None,
        },
        guidance: "Invoke with positional `<ID>` and optional positional input string — never `--input` or `--prompt` (create/update flags). If it returns `Executing`, poll `agent latest-execution <ID>`.",
    },
    DomainCommandContract {
        domain: "agent",
        action: "latest-execution",
        side_effect: DomainCommandSideEffect::ReadOnly,
        primary_json: None,
        key_params: &[DomainParamSummary {
            name: "<id>",
            required: true,
            summary: "Agent ID (positional; discover via `agent list` by name)",
        }],
        shell_example: DomainShellExample {
            command: "neomind agent latest-execution agent-001",
            params_json: None,
        },
        guidance: "For \"is X OK / current state of what an agent watches\": `agent list` then `latest-execution <ID>` — do not invoke for a status question. Prefer this over empty device inventory when a matching agent exists.",
    },
    DomainCommandContract {
        domain: "agent",
        action: "delete",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[DomainParamSummary {
            name: "<id>",
            required: true,
            summary: "Agent ID (positional; discover via `agent list`)",
        }],
        shell_example: DomainShellExample {
            command: "neomind agent delete agent-001",
            params_json: None,
        },
        guidance: "When the user asks to create→run→delete, finish with `agent delete <ID>` (and delete every duplicate create if you minted more than one).",
    },
    // === Widget ===
    DomainCommandContract {
        domain: "widget",
        action: "create",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[
            DomainParamSummary {
                name: "<name>",
                required: true,
                summary: "Widget display name (positional — NOT `--name`)",
            },
            DomainParamSummary {
                name: "--widget-type",
                required: false,
                summary: "chart | gauge | stat | table | image | custom (default custom)",
            },
            DomainParamSummary {
                name: "--output",
                required: false,
                summary: "Output directory for scaffolded files",
            },
            DomainParamSummary {
                name: "--install",
                required: false,
                summary: "Scaffold and register in one step (skips separate `widget install`)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind widget create \"Battery Gauge\" --widget-type gauge --install",
            params_json: None,
        },
        guidance: "Pass the widget name as a positional argument (not `--name`); use `--widget-type` + optional `--install` to scaffold and register.",
    },
    DomainCommandContract {
        domain: "widget",
        action: "install",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[
            DomainParamSummary {
                name: "<file>",
                required: true,
                summary: "Path to scaffolded widget directory or .zip (positional)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind widget install ./battery-gauge",
            params_json: None,
        },
        guidance: "Install from a directory produced by `widget create` (manifest.json + bundle.js) or a .zip; quote paths that contain spaces.",
    },
    // === Extension ===
    DomainCommandContract {
        domain: "extension",
        action: "status",
        side_effect: DomainCommandSideEffect::ReadOnly,
        primary_json: None,
        key_params: &[DomainParamSummary {
            name: "<id>",
            required: false,
            summary: "Extension ID (positional; omit to list all with status)",
        }],
        shell_example: DomainShellExample {
            command: "neomind extension status weather-forecast",
            params_json: None,
        },
        guidance: "Prefer `extension status <ID>` (positional). Omitting `<ID>` lists installed extensions with status — then call again with the target id. Do not invent flags.",
    },
    DomainCommandContract {
        domain: "extension",
        action: "logs",
        side_effect: DomainCommandSideEffect::ReadOnly,
        primary_json: None,
        key_params: &[
            DomainParamSummary {
                name: "<id>",
                required: false,
                summary: "Extension ID (positional; omit to list ids first)",
            },
            DomainParamSummary {
                name: "--lines",
                required: false,
                summary: "Max log lines to return",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind extension logs weather-forecast",
            params_json: None,
        },
        guidance: "Prefer `extension logs <ID> [--lines N]`. Omitting `<ID>` lists installed extensions and tips the next call — then pass the id.",
    },
    DomainCommandContract {
        domain: "extension",
        action: "install",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[DomainParamSummary {
            name: "<file>",
            required: true,
            summary: "Path to `.nep` package (positional)",
        }],
        shell_example: DomainShellExample {
            command: "neomind extension install ./weather-forecast.nep",
            params_json: None,
        },
        guidance: "Install from a local `.nep` path; marketplace installs use `extension market-install <id>` instead.",
    },
    // === Settings ===
    DomainCommandContract {
        domain: "settings",
        action: "set-retention",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[
            DomainParamSummary {
                name: "--interval-hours",
                required: true,
                summary: "Cleanup interval in hours (> 0)",
            },
            DomainParamSummary {
                name: "--enabled",
                required: false,
                summary: "Flag to enable automatic cleanup (omit to leave disabled)",
            },
            DomainParamSummary {
                name: "--default-retention",
                required: false,
                summary: "Telemetry retention limit in hours",
            },
            DomainParamSummary {
                name: "--image-retention",
                required: false,
                summary: "Image retention limit in hours",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind settings set-retention --enabled --interval-hours 24 --default-retention 168",
            params_json: None,
        },
        guidance: "Required `--interval-hours`; use flag `--enabled` (not `--days` / invented flags). Inspect current values with `settings retention` first.",
    },
    DomainCommandContract {
        domain: "settings",
        action: "retention",
        side_effect: DomainCommandSideEffect::ReadOnly,
        primary_json: None,
        key_params: &[],
        shell_example: DomainShellExample {
            command: "neomind settings retention",
            params_json: None,
        },
        guidance: "Read-only: returns current retention config. Change with `settings set-retention`.",
    },
    DomainCommandContract {
        domain: "settings",
        action: "cleanup",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[],
        shell_example: DomainShellExample {
            command: "neomind settings cleanup",
            params_json: None,
        },
        guidance: "Trigger an immediate retention cleanup; review limits with `settings retention` first.",
    },
    // === Connector ===
    DomainCommandContract {
        domain: "connector",
        action: "create",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[
            DomainParamSummary {
                name: "--name",
                required: true,
                summary: "Connector display name",
            },
            DomainParamSummary {
                name: "--host",
                required: true,
                summary: "Broker hostname or IP",
            },
            DomainParamSummary {
                name: "--port",
                required: false,
                summary: "Broker port (default 1883)",
            },
            DomainParamSummary {
                name: "--connector-type",
                required: false,
                summary: "Connector type (mqtt only today; default mqtt)",
            },
            DomainParamSummary {
                name: "--topics",
                required: false,
                summary: "Comma-separated MQTT topic subscriptions",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind connector create --name \"Factory MQTT\" --host 192.168.1.100 --port 1883 --topics \"sensor/#\"",
            params_json: None,
        },
        guidance: "Required `--name` and `--host`; optional `--port` / `--topics` / `--tls` / `--connector-type` (default mqtt). Outbound data targets use `push`, notification channels use `message`.",
    },
    DomainCommandContract {
        domain: "connector",
        action: "enable",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[DomainParamSummary {
            name: "<id>",
            required: true,
            summary: "Connector ID (positional; discover via `connector list`)",
        }],
        shell_example: DomainShellExample {
            command: "neomind connector enable connector-001",
            params_json: None,
        },
        guidance: "Pass the connector ID positionally (`connector enable <ID>`); create first if none exist.",
    },
    // === Message ===
    DomainCommandContract {
        domain: "message",
        action: "channel-create",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--config",
            kind: ExpectedJsonKind::Object,
        }),
        key_params: &[
            DomainParamSummary {
                name: "--name",
                required: true,
                summary: "Unique channel name",
            },
            DomainParamSummary {
                name: "--channel-type",
                required: true,
                summary: "Channel type (`--type` alias); discover via `message channel-types`",
            },
            DomainParamSummary {
                name: "--config",
                required: false,
                summary: "Type-specific config object (prefer shell `params: {...}`)",
            },
            DomainParamSummary {
                name: "--param",
                required: false,
                summary: "Repeatable `key=value` config fields (merged over `--config`)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind message channel-create --name alerts --channel-type webhook",
            params_json: Some(r#"{"url":"https://example.com/webhook"}"#),
        },
        guidance: "Required `--name` + `--channel-type` (alias `--type`); pass type config via shell `params` (maps to `--config`). Discover schema with `message channel-type-schema <TYPE>`.",
    },
    // === Transform ===
    DomainCommandContract {
        domain: "transform",
        action: "create",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: None,
        key_params: &[
            DomainParamSummary {
                name: "--name",
                required: true,
                summary: "Transform name",
            },
            DomainParamSummary {
                name: "--scope",
                required: true,
                summary: "`global` | `device:<id>` | `device_type:<type>` — never bare `device`",
            },
            DomainParamSummary {
                name: "--code",
                required: true,
                summary: "JavaScript body; value bound to `input` (e.g. `return input.battery * 100`)",
            },
            DomainParamSummary {
                name: "--output-prefix",
                required: false,
                summary: "Prefix for virtual metrics",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind transform create --name batt100 --scope device:sensor-001 --code \"return input.battery * 100\" --output-prefix thousands",
            params_json: None,
        },
        guidance: "Required `--name`, `--scope` (`global` / `device:<id>` / `device_type:<type>` — never bare `device`), and `--code`. Fix scope before rewriting code.",
    },
    DomainCommandContract {
        domain: "transform",
        action: "test-code",
        side_effect: DomainCommandSideEffect::Mutating,
        primary_json: Some(PrimaryJsonSpec {
            flag: "--input",
            kind: ExpectedJsonKind::Object,
        }),
        key_params: &[
            DomainParamSummary {
                name: "--code",
                required: true,
                summary: "JavaScript body to evaluate",
            },
            DomainParamSummary {
                name: "--input",
                required: true,
                summary: "Sample input object (prefer shell `params: {...}`)",
            },
        ],
        shell_example: DomainShellExample {
            command: "neomind transform test-code --code \"return input.battery * 100\"",
            params_json: Some(r#"{"battery":75}"#),
        },
        guidance: "Requires `--code` and `--input` (shell `params` maps to `--input`). Do not pass a transform ID as a positional — `test-code` is offline against sample input.",
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

/// How many argv tokens a global CLI flag occupies at `i` (`1` or `2`).
///
/// Recognizes `-v`/`--verbose`, `--json`, `--format <v>`, and `--format=<v>`
/// at any position after `neomind` (clap `global = true`).
fn global_cli_flag_span(argv: &[String], i: usize) -> Option<usize> {
    let a = argv.get(i)?.as_str();
    match a {
        "-v" | "--verbose" | "--json" => Some(1),
        "--format" => {
            if i + 1 < argv.len() && !argv[i + 1].starts_with('-') {
                Some(2)
            } else {
                Some(1)
            }
        }
        a if a.starts_with("--format=") => Some(1),
        _ => None,
    }
}

/// Index of the domain token after skipping leading global CLI flags.
///
/// Clap accepts `-v`/`--verbose`, `--json`, `--format <v>`, and `--format=<v>`
/// before the domain. Callers that still treat `argv[1]`/`argv[2]` as
/// domain/action must use this (or [`domain_action_from_argv`]) so
/// `neomind --json extension config …` does not mis-classify writes.
pub fn domain_token_index(argv: &[String]) -> Option<usize> {
    if argv.first().map(String::as_str) != Some("neomind") {
        return None;
    }
    let mut i = 1usize;
    while i < argv.len() {
        match global_cli_flag_span(argv, i) {
            Some(skip) => i += skip,
            None => break,
        }
    }
    Some(i)
}

/// `(domain, action)` after skipping global flags anywhere before either token.
///
/// Handles both `neomind --json extension config …` and
/// `neomind extension --json config …` (globals between domain and action).
/// Returns `None` when argv is too short or not a `neomind` command.
pub fn domain_action_from_argv(argv: &[String]) -> Option<(&str, &str)> {
    if argv.first().map(String::as_str) != Some("neomind") {
        return None;
    }
    let mut i = 1usize;
    while i < argv.len() {
        match global_cli_flag_span(argv, i) {
            Some(skip) => i += skip,
            None => break,
        }
    }
    let domain = argv.get(i)?.as_str();
    if domain.starts_with('-') {
        return None;
    }
    i += 1;
    while i < argv.len() {
        match global_cli_flag_span(argv, i) {
            Some(skip) => i += skip,
            None => break,
        }
    }
    let action = argv.get(i)?.as_str();
    if action.starts_with('-') {
        return None;
    }
    Some((domain, action))
}

/// Rebuild argv as `["neomind", <domain>, <action>, …]` with global flags
/// removed from any position (`-v`/`--json`/`--format` …).
pub fn strip_global_cli_flags(argv: &[String]) -> Vec<String> {
    if argv.first().map(String::as_str) != Some("neomind") {
        return argv.to_vec();
    }
    let mut out = Vec::with_capacity(argv.len());
    out.push("neomind".to_string());
    let mut i = 1usize;
    while i < argv.len() {
        if let Some(skip) = global_cli_flag_span(argv, i) {
            i += skip;
            continue;
        }
        out.push(argv[i].clone());
        i += 1;
    }
    out
}

/// Rebuild argv with leading globals removed (alias of [`strip_global_cli_flags`]).
pub fn strip_leading_global_cli_flags(argv: &[String]) -> Vec<String> {
    strip_global_cli_flags(argv)
}

/// Look up a core [`DomainCommandContract`] for a tokenized `neomind <domain> <action> ...` argv.
pub fn find_domain_command_contract_for_argv(
    argv: &[String],
) -> Option<&'static DomainCommandContract> {
    let (domain, action) = domain_action_from_argv(argv)?;
    find_domain_command_contract(domain, action)
}

/// Return all registered core domain command contracts.
pub fn core_domain_contracts() -> &'static [DomainCommandContract] {
    CORE_DOMAIN_CONTRACTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_contracts_cover_ten_business_domains() {
        let required_mutating = [
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
            ("agent", "delete"),
            ("widget", "create"),
            ("widget", "install"),
            ("extension", "install"),
            ("settings", "set-retention"),
            ("settings", "cleanup"),
            ("connector", "create"),
            ("connector", "enable"),
            ("message", "channel-create"),
            ("transform", "create"),
            ("transform", "test-code"),
        ];
        let required_readonly = [
            ("extension", "status"),
            ("extension", "logs"),
            ("settings", "retention"),
        ];
        for (domain, action) in required_mutating {
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
        for (domain, action) in required_readonly {
            let c = find_domain_command_contract(domain, action)
                .unwrap_or_else(|| panic!("missing contract for `{domain} {action}`"));
            assert_eq!(c.side_effect, DomainCommandSideEffect::ReadOnly);
        }
        let domains: std::collections::HashSet<&str> =
            CORE_DOMAIN_CONTRACTS.iter().map(|c| c.domain).collect();
        for d in [
            "device",
            "rule",
            "dashboard",
            "agent",
            "widget",
            "extension",
            "settings",
            "connector",
            "message",
            "transform",
        ] {
            assert!(domains.contains(d), "missing domain `{d}` in contracts");
        }
        assert_eq!(
            domains.len(),
            10,
            "expected exactly 10 domains, got {domains:?}"
        );
    }

    #[test]
    fn domain_action_skips_leading_global_cli_flags() {
        let argv =
            |parts: &[&str]| -> Vec<String> { parts.iter().map(|s| (*s).to_string()).collect() };
        assert_eq!(
            domain_action_from_argv(&argv(&["neomind", "extension", "config"])),
            Some(("extension", "config"))
        );
        assert_eq!(
            domain_action_from_argv(&argv(&[
                "neomind",
                "--json",
                "extension",
                "config",
                "weather"
            ])),
            Some(("extension", "config"))
        );
        assert_eq!(
            domain_action_from_argv(&argv(&[
                "neomind", "--format", "json", "-v", "rule", "create"
            ])),
            Some(("rule", "create"))
        );
        assert_eq!(
            domain_action_from_argv(&argv(&["neomind", "--format=json", "transform", "list"])),
            Some(("transform", "list"))
        );
        // Globals between domain and action (clap `global = true`).
        assert_eq!(
            domain_action_from_argv(&argv(&[
                "neomind",
                "extension",
                "--json",
                "config",
                "weather"
            ])),
            Some(("extension", "config"))
        );
        assert_eq!(
            domain_action_from_argv(&argv(&[
                "neomind",
                "extension",
                "--format",
                "json",
                "-v",
                "config",
                "weather"
            ])),
            Some(("extension", "config"))
        );
        let stripped = strip_global_cli_flags(&argv(&[
            "neomind",
            "extension",
            "--json",
            "config",
            "weather",
        ]));
        assert_eq!(
            stripped,
            vec![
                "neomind".to_string(),
                "extension".to_string(),
                "config".to_string(),
                "weather".to_string()
            ]
        );
        let stripped_leading = strip_leading_global_cli_flags(&argv(&[
            "neomind",
            "--json",
            "extension",
            "config",
            "weather",
        ]));
        assert_eq!(
            stripped_leading,
            vec![
                "neomind".to_string(),
                "extension".to_string(),
                "config".to_string(),
                "weather".to_string()
            ]
        );
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
