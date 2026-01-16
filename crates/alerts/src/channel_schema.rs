//! Alert channel configuration schemas.
//!
//! This module defines the JSON Schema for each built-in channel type,
//! enabling dynamic configuration UI generation.

use serde_json::json;

/// Get the configuration schema for a channel type.
pub fn get_channel_schema(channel_type: &str) -> Option<serde_json::Value> {
    match channel_type {
        "console" => Some(console_channel_schema()),
        "memory" => Some(memory_channel_schema()),
        "webhook" => Some(webhook_channel_schema()),
        "email" => Some(email_channel_schema()),
        _ => None,
    }
}

/// List all available channel types with their metadata.
pub fn list_channel_types() -> Vec<ChannelTypeInfo> {
    vec![
        ChannelTypeInfo {
            id: "console".to_string(),
            name: "Console".to_string(),
            name_zh: "控制台".to_string(),
            description: "Print alerts to console output".to_string(),
            description_zh: "将告警打印到控制台输出".to_string(),
            icon: "terminal".to_string(),
            category: "builtin".to_string(),
        },
        ChannelTypeInfo {
            id: "memory".to_string(),
            name: "Memory".to_string(),
            name_zh: "内存".to_string(),
            description: "Store alerts in memory for testing".to_string(),
            description_zh: "将告警存储在内存中用于测试".to_string(),
            icon: "database".to_string(),
            category: "builtin".to_string(),
        },
        ChannelTypeInfo {
            id: "webhook".to_string(),
            name: "Webhook".to_string(),
            name_zh: "Webhook".to_string(),
            description: "Send alerts via HTTP POST webhook".to_string(),
            description_zh: "通过 HTTP POST Webhook 发送告警".to_string(),
            icon: "webhook".to_string(),
            category: "notification".to_string(),
        },
        ChannelTypeInfo {
            id: "email".to_string(),
            name: "Email".to_string(),
            name_zh: "邮件".to_string(),
            description: "Send alerts via email SMTP".to_string(),
            description_zh: "通过 SMTP 邮件发送告警".to_string(),
            icon: "mail".to_string(),
            category: "notification".to_string(),
        },
    ]
}

/// Metadata about a channel type.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChannelTypeInfo {
    /// Channel type identifier
    pub id: String,
    /// English name
    pub name: String,
    /// Chinese name
    pub name_zh: String,
    /// English description
    pub description: String,
    /// Chinese description
    pub description_zh: String,
    /// Icon name
    pub icon: String,
    /// Category (builtin, notification, etc.)
    pub category: String,
}

/// Console channel configuration schema.
fn console_channel_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "title": "Console Channel",
        "title_zh": "控制台通道",
        "description": "Configuration for console output channel",
        "description_zh": "控制台输出通道配置",
        "properties": {
            "name": {
                "type": "string",
                "title": "Channel Name",
                "title_zh": "通道名称",
                "description": "Unique identifier for this channel",
                "description_zh": "通道的唯一标识符",
                "default": "console",
                "minLength": 1,
                "maxLength": 50
            },
            "enabled": {
                "type": "boolean",
                "title": "Enabled",
                "title_zh": "启用",
                "description": "Whether this channel is active",
                "description_zh": "是否启用此通道",
                "default": true
            },
            "include_details": {
                "type": "boolean",
                "title": "Include Details",
                "title_zh": "包含详细信息",
                "description": "Show alert metadata in output",
                "description_zh": "在输出中显示告警元数据",
                "default": true
            }
        },
        "required": ["name"],
        "ui_hints": {
            "field_order": ["name", "enabled", "include_details"],
            "display_names": {
                "name": "Channel Name",
                "name_zh": "通道名称",
                "enabled": "Enabled",
                "enabled_zh": "启用",
                "include_details": "Include Details",
                "include_details_zh": "包含详细信息"
            },
            "placeholders": {
                "name": "e.g., console, main-console"
            }
        }
    })
}

/// Memory channel configuration schema.
fn memory_channel_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "title": "Memory Channel",
        "title_zh": "内存通道",
        "description": "Configuration for in-memory alert storage",
        "description_zh": "内存告警存储配置",
        "properties": {
            "name": {
                "type": "string",
                "title": "Channel Name",
                "title_zh": "通道名称",
                "description": "Unique identifier for this channel",
                "description_zh": "通道的唯一标识符",
                "default": "memory",
                "minLength": 1,
                "maxLength": 50
            },
            "enabled": {
                "type": "boolean",
                "title": "Enabled",
                "title_zh": "启用",
                "description": "Whether this channel is active",
                "description_zh": "是否启用此通道",
                "default": true
            },
            "max_alerts": {
                "type": "integer",
                "title": "Max Alerts",
                "title_zh": "最大告警数",
                "description": "Maximum number of alerts to store (0 = unlimited)",
                "description_zh": "存储的最大告警数量（0 = 无限制）",
                "default": 1000,
                "minimum": 0,
                "maximum": 100000
            }
        },
        "required": ["name"],
        "ui_hints": {
            "field_order": ["name", "enabled", "max_alerts"],
            "display_names": {
                "name": "Channel Name",
                "name_zh": "通道名称",
                "enabled": "Enabled",
                "enabled_zh": "启用",
                "max_alerts": "Max Alerts",
                "max_alerts_zh": "最大告警数"
            }
        }
    })
}

/// Webhook channel configuration schema.
fn webhook_channel_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "title": "Webhook Channel",
        "title_zh": "Webhook 通道",
        "description": "Configuration for HTTP webhook notifications",
        "description_zh": "HTTP Webhook 通知配置",
        "properties": {
            "name": {
                "type": "string",
                "title": "Channel Name",
                "title_zh": "通道名称",
                "description": "Unique identifier for this channel",
                "description_zh": "通道的唯一标识符",
                "default": "webhook",
                "minLength": 1,
                "maxLength": 50
            },
            "enabled": {
                "type": "boolean",
                "title": "Enabled",
                "title_zh": "启用",
                "description": "Whether this channel is active",
                "description_zh": "是否启用此通道",
                "default": true
            },
            "url": {
                "type": "string",
                "title": "Webhook URL",
                "title_zh": "Webhook 地址",
                "description": "HTTP endpoint URL for sending alerts",
                "description_zh": "发送告警的 HTTP 端点 URL",
                "format": "uri",
                "default": "https://example.com/webhook"
            },
            "method": {
                "type": "string",
                "title": "HTTP Method",
                "title_zh": "HTTP 方法",
                "description": "HTTP method to use",
                "description_zh": "使用的 HTTP 方法",
                "enum": ["POST", "PUT", "PATCH"],
                "default": "POST"
            },
            "headers": {
                "type": "object",
                "title": "Headers",
                "title_zh": "请求头",
                "description": "Additional HTTP headers",
                "description_zh": "额外的 HTTP 请求头",
                "default": {},
                "additionalProperties": {"type": "string"}
            },
            "timeout_seconds": {
                "type": "integer",
                "title": "Timeout (seconds)",
                "title_zh": "超时时间（秒）",
                "description": "Request timeout in seconds",
                "description_zh": "请求超时时间（秒）",
                "default": 30,
                "minimum": 1,
                "maximum": 300
            },
            "retry_attempts": {
                "type": "integer",
                "title": "Retry Attempts",
                "title_zh": "重试次数",
                "description": "Number of retry attempts on failure",
                "description_zh": "失败时的重试次数",
                "default": 3,
                "minimum": 0,
                "maximum": 10
            }
        },
        "required": ["name", "url"],
        "ui_hints": {
            "field_order": ["name", "enabled", "url", "method", "headers", "timeout_seconds", "retry_attempts"],
            "display_names": {
                "name": "Channel Name",
                "name_zh": "通道名称",
                "enabled": "Enabled",
                "enabled_zh": "启用",
                "url": "Webhook URL",
                "url_zh": "Webhook 地址",
                "method": "HTTP Method",
                "method_zh": "HTTP 方法",
                "headers": "Headers",
                "headers_zh": "请求头",
                "timeout_seconds": "Timeout",
                "timeout_seconds_zh": "超时时间",
                "retry_attempts": "Retry Attempts",
                "retry_attempts_zh": "重试次数"
            },
            "placeholders": {
                "url": "https://hooks.slack.com/services/...",
                "headers": "{\"Authorization\": \"Bearer token\"}"
            },
            "help_texts": {
                "headers": "JSON object with key-value pairs for HTTP headers",
                "headers_zh": "包含 HTTP 请求头的 JSON 对象"
            }
        }
    })
}

/// Email channel configuration schema.
fn email_channel_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "title": "Email Channel",
        "title_zh": "邮件通道",
        "description": "Configuration for email SMTP notifications",
        "description_zh": "SMTP 邮件通知配置",
        "properties": {
            "name": {
                "type": "string",
                "title": "Channel Name",
                "title_zh": "通道名称",
                "description": "Unique identifier for this channel",
                "description_zh": "通道的唯一标识符",
                "default": "email",
                "minLength": 1,
                "maxLength": 50
            },
            "enabled": {
                "type": "boolean",
                "title": "Enabled",
                "title_zh": "启用",
                "description": "Whether this channel is active",
                "description_zh": "是否启用此通道",
                "default": true
            },
            "smtp_server": {
                "type": "string",
                "title": "SMTP Server",
                "title_zh": "SMTP 服务器",
                "description": "SMTP server hostname or IP",
                "description_zh": "SMTP 服务器主机名或 IP",
                "default": "smtp.gmail.com"
            },
            "smtp_port": {
                "type": "integer",
                "title": "SMTP Port",
                "title_zh": "SMTP 端口",
                "description": "SMTP server port",
                "description_zh": "SMTP 服务器端口",
                "default": 587,
                "enum": [25, 465, 587, 2525],
                "enum_titles": ["25 (Plain)", "465 (SSL)", "587 (STARTTLS)", "2525 (Alternative)"]
            },
            "username": {
                "type": "string",
                "title": "Username",
                "title_zh": "用户名",
                "description": "SMTP authentication username",
                "description_zh": "SMTP 认证用户名",
                "default": ""
            },
            "password": {
                "type": "string",
                "title": "Password",
                "title_zh": "密码",
                "description": "SMTP authentication password",
                "description_zh": "SMTP 认证密码",
                "x-secret": true,
                "default": ""
            },
            "from_address": {
                "type": "string",
                "title": "From Address",
                "title_zh": "发件人地址",
                "description": "Sender email address",
                "description_zh": "发件人邮箱地址",
                "format": "email",
                "default": "noreply@example.com"
            },
            "to_addresses": {
                "type": "array",
                "title": "Recipients",
                "title_zh": "收件人",
                "description": "List of recipient email addresses",
                "description_zh": "收件人邮箱地址列表",
                "items": {
                    "type": "string",
                    "format": "email"
                },
                "minItems": 1,
                "default": ["admin@example.com"]
            },
            "use_tls": {
                "type": "boolean",
                "title": "Use TLS",
                "title_zh": "使用 TLS",
                "description": "Use TLS/SSL for secure connection",
                "description_zh": "使用 TLS/SSL 安全连接",
                "default": true
            },
            "subject_prefix": {
                "type": "string",
                "title": "Subject Prefix",
                "title_zh": "主题前缀",
                "description": "Prefix for email subject",
                "description_zh": "邮件主题前缀",
                "default": "[Alert]"
            }
        },
        "required": ["name", "smtp_server", "smtp_port", "from_address", "to_addresses"],
        "ui_hints": {
            "field_order": [
                "name", "enabled", "smtp_server", "smtp_port", "use_tls",
                "username", "password", "from_address", "to_addresses", "subject_prefix"
            ],
            "display_names": {
                "name": "Channel Name",
                "name_zh": "通道名称",
                "enabled": "Enabled",
                "enabled_zh": "启用",
                "smtp_server": "SMTP Server",
                "smtp_server_zh": "SMTP 服务器",
                "smtp_port": "SMTP Port",
                "smtp_port_zh": "SMTP 端口",
                "use_tls": "Use TLS",
                "use_tls_zh": "使用 TLS",
                "username": "Username",
                "username_zh": "用户名",
                "password": "Password",
                "password_zh": "密码",
                "from_address": "From Address",
                "from_address_zh": "发件人地址",
                "to_addresses": "Recipients",
                "to_addresses_zh": "收件人",
                "subject_prefix": "Subject Prefix",
                "subject_prefix_zh": "主题前缀"
            },
            "placeholders": {
                "smtp_server": "smtp.gmail.com",
                "username": "your-email@gmail.com",
                "from_address": "noreply@yourdomain.com"
            },
            "help_texts": {
                "to_addresses": "Comma-separated list of email addresses",
                "to_addresses_zh": "逗号分隔的邮箱地址列表"
            },
            "visibility_rules": [
                {
                    "field": "smtp_port",
                    "condition": "equals",
                    "value": 465,
                    "then_show": ["use_tls"],
                    "then_hide": [],
                    "then_disable": ["use_tls"]
                }
            ]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_console_schema() {
        let schema = console_channel_schema();
        assert_eq!(schema["title"], "Console Channel");
        assert!(schema["properties"]["name"].is_object());
        assert!(schema["required"].is_array());
    }

    #[test]
    fn test_webhook_schema() {
        let schema = webhook_channel_schema();
        assert_eq!(schema["title"], "Webhook Channel");
        assert!(schema["properties"]["url"].is_object());
        assert!(schema["required"].as_array().unwrap().contains(&json!("url")));
    }

    #[test]
    fn test_email_schema() {
        let schema = email_channel_schema();
        assert_eq!(schema["title"], "Email Channel");
        assert!(schema["properties"]["smtp_server"].is_object());
        assert!(schema["properties"]["password"]["x-secret"] == true);
    }

    #[test]
    fn test_list_channel_types() {
        let types = list_channel_types();
        assert_eq!(types.len(), 4);
        assert!(types.iter().any(|t| t.id == "email"));
        assert!(types.iter().any(|t| t.id == "webhook"));
    }
}
