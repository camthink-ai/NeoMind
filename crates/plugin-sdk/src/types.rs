//! Common types for plugins.

use serde_json::Value;

/// Plugin context provides runtime information to plugins
#[derive(Debug, Clone)]
pub struct PluginContext {
    /// Plugin ID
    pub plugin_id: String,

    /// Plugin configuration
    pub config: Value,

    /// Base directory for plugin data
    pub data_dir: Option<String>,

    /// Temporary directory
    pub temp_dir: Option<String>,
}

impl PluginContext {
    /// Create a new plugin context
    pub fn new(plugin_id: impl Into<String>, config: Value) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            config,
            data_dir: None,
            temp_dir: None,
        }
    }

    /// Get a configuration value by key
    pub fn get_config(&self, key: &str) -> Option<&Value> {
        self.config.get(key)
    }

    /// Get a configuration value as string
    pub fn get_config_str(&self, key: &str) -> Option<&str> {
        self.config.get(key)?.as_str()
    }

    /// Get a configuration value as number
    pub fn get_config_number(&self, key: &str) -> Option<f64> {
        self.config.get(key)?.as_f64()
    }

    /// Get a configuration value as bool
    pub fn get_config_bool(&self, key: &str) -> Option<bool> {
        self.config.get(key)?.as_bool()
    }
}

/// A request from the host to the plugin
#[derive(Debug, Clone)]
pub struct PluginRequest {
    /// Request type/command
    pub command: String,

    /// Request arguments
    pub args: Value,

    /// Request ID for tracking
    pub request_id: Option<String>,
}

impl PluginRequest {
    /// Create a new request
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Value::Object(Default::default()),
            request_id: None,
        }
    }

    /// Set the request arguments
    pub fn with_args(mut self, args: Value) -> Self {
        self.args = args;
        self
    }

    /// Set the request ID
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    /// Get an argument by key
    pub fn get_arg(&self, key: &str) -> Option<&Value> {
        self.args.get(key)
    }
}

/// A response from the plugin to the host
#[derive(Debug, Clone)]
pub struct PluginResponse {
    /// Response data
    pub data: Value,

    /// Whether the request was successful
    pub success: bool,

    /// Error message if not successful
    pub error: Option<String>,

    /// Additional metadata
    pub metadata: Value,
}

impl PluginResponse {
    /// Create a successful response
    pub fn success(data: Value) -> Self {
        Self {
            data,
            success: true,
            error: None,
            metadata: Value::Object(Default::default()),
        }
    }

    /// Create an error response
    pub fn error(error: impl Into<String>) -> Self {
        Self {
            data: Value::Null,
            success: false,
            error: Some(error.into()),
            metadata: Value::Object(Default::default()),
        }
    }

    /// Add metadata to the response
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        if let Value::Object(ref mut map) = self.metadata {
            map.insert(key.into(), value);
        }
        self
    }
}

impl From<Value> for PluginResponse {
    fn from(data: Value) -> Self {
        Self::success(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_config_access() {
        let config = serde_json::json!({
            "api_key": "secret",
            "timeout": 30,
            "enabled": true
        });

        let ctx = PluginContext::new("test-plugin", config);

        assert_eq!(ctx.get_config_str("api_key"), Some("secret"));
        assert_eq!(ctx.get_config_number("timeout"), Some(30.0));
        assert_eq!(ctx.get_config_bool("enabled"), Some(true));
    }

    #[test]
    fn test_response_creation() {
        let success = PluginResponse::success(serde_json::json!({"result": "ok"}));
        assert!(success.success);
        assert!(success.error.is_none());

        let error = PluginResponse::error("something went wrong");
        assert!(!error.success);
        assert_eq!(error.error, Some("something went wrong".to_string()));
    }
}
