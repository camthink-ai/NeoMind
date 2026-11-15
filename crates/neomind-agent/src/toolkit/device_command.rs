//! Structured device command tool — device id + command + params JSON.
//!
//! Parallel to shell `neomind device control …` (CLI kept). Discovers command
//! parameter schemas from the device type template — no device-type hardcoding.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::error::Result;
use super::tool::{Tool, ToolCategory, ToolOutput};

/// Outcome of sending a device command through the platform service.
#[derive(Debug, Clone)]
pub struct DeviceCommandOutcome {
    pub device_id: String,
    pub command: String,
    pub delivery: String,
    pub delivery_note: String,
}

/// Look up a device + its command catalog (for capability discovery / errors).
#[derive(Debug, Clone)]
pub struct DeviceCommandCatalog {
    pub device_id: String,
    pub device_name: String,
    pub device_type: String,
    pub commands: Vec<CommandCapability>,
}

/// One command from the device type template (params discovered, not hardcoded).
#[derive(Debug, Clone)]
pub struct CommandCapability {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub parameters: Vec<ParamCapability>,
}

/// Parameter schema surfaced to the model when discovery / validation fails.
#[derive(Debug, Clone)]
pub struct ParamCapability {
    pub name: String,
    pub display_name: String,
    pub data_type: String,
    pub required: bool,
    pub description: String,
}

/// Resolve device + list template commands (capability discovery).
pub type DiscoverDeviceCommandsCallback = Arc<
    dyn Fn(
            String,
        ) -> Pin<
            Box<dyn Future<Output = std::result::Result<DeviceCommandCatalog, String>> + Send>,
        > + Send
        + Sync,
>;

/// Send a validated command via DeviceService::send_command_reported.
pub type SendDeviceCommandCallback = Arc<
    dyn Fn(
            String,
            String,
            HashMap<String, Value>,
        ) -> Pin<
            Box<dyn Future<Output = std::result::Result<DeviceCommandOutcome, String>> + Send>,
        > + Send
        + Sync,
>;

/// Structured device command entry for chat agents.
pub struct DeviceCommandTool {
    discover: DiscoverDeviceCommandsCallback,
    send: SendDeviceCommandCallback,
}

impl DeviceCommandTool {
    pub fn new(discover: DiscoverDeviceCommandsCallback, send: SendDeviceCommandCallback) -> Self {
        Self { discover, send }
    }
}

fn command_schema_hint(cmd: &CommandCapability) -> Value {
    let params: Vec<Value> = cmd
        .parameters
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "display_name": p.display_name,
                "type": p.data_type,
                "required": p.required,
                "description": p.description,
            })
        })
        .collect();
    json!({
        "command": cmd.name,
        "display_name": cmd.display_name,
        "description": cmd.description,
        "parameters": params,
    })
}

fn catalog_commands_summary(catalog: &DeviceCommandCatalog) -> Value {
    json!({
        "device_id": catalog.device_id,
        "device_name": catalog.device_name,
        "device_type": catalog.device_type,
        "commands": catalog.commands.iter().map(command_schema_hint).collect::<Vec<_>>(),
    })
}

#[async_trait]
impl Tool for DeviceCommandTool {
    fn name(&self) -> &str {
        "device_command"
    }

    fn description(&self) -> &str {
        "Send a native command to a registered device using structured arguments \
         (device id, command name, params object). Prefer this over assembling \
         `neomind device control …` in the shell for ordinary device commands. \
         Parameter names and types come from the device type template — inspect \
         error feedback when a command or params are wrong (lists available \
         commands and their schemas). Delivery is honest: adapter/extension \
         accept is not a device ACK or physical-effect proof. Shell CLI remains \
         available for advanced / non-template operations."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "device": {
                    "type": "string",
                    "description": "Device id (preferred) or exact device name."
                },
                "command": {
                    "type": "string",
                    "description": "Template command name for this device type (e.g. reboot, render_text)."
                },
                "params": {
                    "type": "object",
                    "description": "Command parameters as a JSON object. Keys must match the device type command schema. Omit or pass {} when the command has no params."
                }
            },
            "required": ["device", "command"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let Some(device_needle) = args.get("device").and_then(|v| v.as_str()) else {
            return Ok(ToolOutput::error(
                "Missing required parameter 'device' — pass the device id (or exact name).",
            ));
        };
        let device_needle = device_needle.trim();
        if device_needle.is_empty() {
            return Ok(ToolOutput::error(
                "Parameter 'device' must be a non-empty device id or name.",
            ));
        }

        let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
            return Ok(ToolOutput::error(
                "Missing required parameter 'command' — pass the template command name.",
            ));
        };
        let command = command.trim();
        if command.is_empty() {
            return Ok(ToolOutput::error(
                "Parameter 'command' must be a non-empty template command name.",
            ));
        }

        let params_map: HashMap<String, Value> = match args.get("params") {
            None | Some(Value::Null) => HashMap::new(),
            Some(Value::Object(map)) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            Some(other) => {
                return Ok(ToolOutput::error(format!(
                    "Parameter 'params' must be a JSON object, got {other}."
                )));
            }
        };

        let catalog = match (self.discover)(device_needle.to_string()).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolOutput::error(format!(
                    "Could not resolve device '{device_needle}': {e}"
                )));
            }
        };

        let Some(cmd_cap) = catalog.commands.iter().find(|c| c.name == command) else {
            let available: Vec<&str> = catalog.commands.iter().map(|c| c.name.as_str()).collect();
            return Ok(ToolOutput::error_with_metadata(
                format!(
                    "Command '{command}' is not in the template for device '{}' (type '{}'). Available commands: {}. Use one of these names and match params to the schema.",
                    catalog.device_id,
                    catalog.device_type,
                    if available.is_empty() {
                        "(none)".to_string()
                    } else {
                        available.join(", ")
                    }
                ),
                catalog_commands_summary(&catalog),
            ));
        };

        // Preflight required params so the model sees capability schema without
        // a round-trip that only fails inside DeviceService.
        let missing: Vec<&str> = cmd_cap
            .parameters
            .iter()
            .filter(|p| p.required && !params_map.contains_key(&p.name))
            .map(|p| p.name.as_str())
            .collect();
        if !missing.is_empty() {
            return Ok(ToolOutput::error_with_metadata(
                format!(
                    "Command '{command}' on device '{}' is missing required params: {}. Schema:",
                    catalog.device_id,
                    missing.join(", ")
                ),
                command_schema_hint(cmd_cap),
            ));
        }

        match (self.send)(catalog.device_id.clone(), command.to_string(), params_map).await {
            Ok(outcome) => Ok(ToolOutput::success(json!({
                "sent": true,
                "device_id": outcome.device_id,
                "command": outcome.command,
                "delivery": outcome.delivery,
                "delivery_note": outcome.delivery_note,
                "message": format!(
                    "Command sent; {}",
                    outcome.delivery_note
                ),
            }))),
            Err(e) => {
                // Include schema so the model can correct params / command name.
                Ok(ToolOutput::error_with_metadata(
                    format!(
                        "Failed to send '{command}' to device '{}': {e}",
                        catalog.device_id
                    ),
                    command_schema_hint(cmd_cap),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn sample_catalog() -> DeviceCommandCatalog {
        DeviceCommandCatalog {
            device_id: "cam-1".into(),
            device_name: "Lobby Cam".into(),
            device_type: "generic_actuator".into(),
            commands: vec![
                CommandCapability {
                    name: "reboot".into(),
                    display_name: "Reboot".into(),
                    description: "Restart device".into(),
                    parameters: vec![],
                },
                CommandCapability {
                    name: "render_text".into(),
                    display_name: "Render Text".into(),
                    description: "Show text on the device display".into(),
                    parameters: vec![ParamCapability {
                        name: "text".into(),
                        display_name: "Text".into(),
                        data_type: "string".into(),
                        required: true,
                        description: "Content to show".into(),
                    }],
                },
                CommandCapability {
                    name: "set_config".into(),
                    display_name: "Set Config".into(),
                    description: "Update a config key".into(),
                    parameters: vec![
                        ParamCapability {
                            name: "key".into(),
                            display_name: "Key".into(),
                            data_type: "string".into(),
                            required: true,
                            description: "Config key".into(),
                        },
                        ParamCapability {
                            name: "value".into(),
                            display_name: "Value".into(),
                            data_type: "string".into(),
                            required: true,
                            description: "Config value".into(),
                        },
                    ],
                },
            ],
        }
    }

    fn tool_ok() -> DeviceCommandTool {
        let discover: DiscoverDeviceCommandsCallback = Arc::new(|needle| {
            Box::pin(async move {
                let mut c = sample_catalog();
                if needle == "cam-1" || needle == "Lobby Cam" {
                    Ok(c)
                } else if needle == "speaker-9" {
                    c.device_id = "speaker-9".into();
                    c.device_name = "Hall Speaker".into();
                    c.device_type = "audio_player".into();
                    c.commands = vec![CommandCapability {
                        name: "play_cue".into(),
                        display_name: "Play Cue".into(),
                        description: "Play a named cue".into(),
                        parameters: vec![ParamCapability {
                            name: "cue".into(),
                            display_name: "Cue".into(),
                            data_type: "string".into(),
                            required: true,
                            description: "Cue id".into(),
                        }],
                    }];
                    Ok(c)
                } else {
                    Err(format!("device '{needle}' not found"))
                }
            })
        });
        let send: SendDeviceCommandCallback = Arc::new(|device_id, command, params| {
            Box::pin(async move {
                Ok(DeviceCommandOutcome {
                    device_id,
                    command: format!("{command}:{}", params.len()),
                    delivery: "AdapterAccepted".into(),
                    delivery_note: "delivered to 1 adapter(s) (broker publish); device acknowledgment: none; physical effect unverified".into(),
                })
            })
        });
        DeviceCommandTool::new(discover, send)
    }

    #[tokio::test]
    async fn schema_requires_device_and_command() {
        let t = tool_ok();
        let schema = t.parameters();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "device"));
        assert!(required.iter().any(|v| v == "command"));
        assert_eq!(t.name(), "device_command");
    }

    #[tokio::test]
    async fn missing_args_error() {
        let t = tool_ok();
        let out = t.execute(json!({})).await.unwrap();
        assert!(!out.success);
        assert!(out.error.as_ref().unwrap().contains("device"));
    }

    #[tokio::test]
    async fn unknown_command_lists_capability_schema() {
        let t = tool_ok();
        let out = t
            .execute(json!({"device":"cam-1","command":"not_a_real_cmd"}))
            .await
            .unwrap();
        assert!(!out.success);
        assert!(out.error.as_ref().unwrap().contains("Available commands"));
        assert!(out.data.get("commands").is_some());
        let cmds = out.data["commands"].as_array().unwrap();
        assert!(cmds.iter().any(|c| c["command"] == "render_text"));
        assert!(cmds.iter().any(|c| c["command"] == "set_config"));
    }

    #[tokio::test]
    async fn missing_required_params_returns_schema() {
        let t = tool_ok();
        let out = t
            .execute(json!({"device":"cam-1","command":"render_text","params":{}}))
            .await
            .unwrap();
        assert!(!out.success);
        assert!(out.error.as_ref().unwrap().contains("required params"));
        assert_eq!(out.data["command"], "render_text");
        assert!(out.data["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "text" && p["required"] == true));
    }

    #[tokio::test]
    async fn varied_devices_commands_and_text_content() {
        let t = tool_ok();

        // No-param command
        let reboot = t
            .execute(json!({"device":"cam-1","command":"reboot"}))
            .await
            .unwrap();
        assert!(reboot.success);
        assert_eq!(reboot.data["sent"], true);
        assert!(reboot.data["delivery_note"]
            .as_str()
            .unwrap()
            .contains("physical effect unverified"));

        // Text content param
        let text = t
            .execute(json!({
                "device":"Lobby Cam",
                "command":"render_text",
                "params":{"text":"Hello lobby — 天气晴"}
            }))
            .await
            .unwrap();
        assert!(text.success);
        assert_eq!(text.data["device_id"], "cam-1");

        // Different device type + command
        let cue = t
            .execute(json!({
                "device":"speaker-9",
                "command":"play_cue",
                "params":{"cue":"chime-open"}
            }))
            .await
            .unwrap();
        assert!(cue.success);
        assert_eq!(cue.data["device_id"], "speaker-9");

        // Config-style command
        let cfg = t
            .execute(json!({
                "device":"cam-1",
                "command":"set_config",
                "params":{"key":"brightness","value":"80"}
            }))
            .await
            .unwrap();
        assert!(cfg.success);
    }

    #[tokio::test]
    async fn unknown_device_errors() {
        let t = tool_ok();
        let out = t
            .execute(json!({"device":"no-such","command":"reboot"}))
            .await
            .unwrap();
        assert!(!out.success);
        assert!(out.error.as_ref().unwrap().contains("not found"));
    }
}
