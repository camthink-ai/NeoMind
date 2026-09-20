use serde::{Deserialize, Serialize};

/// Structured output for all CLI commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Recovery hint to help LLM correct its next action.
    /// e.g. "Run 'neomind device list' to see available devices"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl CliResponse {
    pub fn success(data: serde_json::Value, message: impl Into<String>) -> Self {
        Self {
            success: true,
            data: Some(data),
            message: Some(message.into()),
            error: None,
            code: None,
            suggestion: None,
        }
    }

    pub fn error(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            message: None,
            error: Some(error.into()),
            code: Some(code.into()),
            suggestion: None,
        }
    }

    pub fn error_with_suggestion(
        error: impl Into<String>,
        code: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            success: false,
            data: None,
            message: None,
            error: Some(error.into()),
            code: Some(code.into()),
            suggestion: Some(suggestion.into()),
        }
    }

    /// Add a suggestion to an existing response (mutates in place).
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

/// Output format control
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Human,
    Json,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_cli_response_success() {
        let data = json!({"id": "123", "name": "test"});
        let response = CliResponse::success(data.clone(), "Operation successful");

        assert!(response.success);
        assert_eq!(response.message, Some("Operation successful".to_string()));
        assert_eq!(response.data, Some(data));
        assert!(response.error.is_none());
        assert!(response.code.is_none());
        assert!(response.suggestion.is_none());
    }

    #[test]
    fn test_cli_response_error() {
        let response = CliResponse::error("Something went wrong", "ERR_001");

        assert!(!response.success);
        assert_eq!(response.error, Some("Something went wrong".to_string()));
        assert_eq!(response.code, Some("ERR_001".to_string()));
        assert!(response.data.is_none());
        assert!(response.message.is_none());
        assert!(response.suggestion.is_none());
    }

    #[test]
    fn test_cli_response_error_with_suggestion() {
        let response = CliResponse::error_with_suggestion(
            "Device not found",
            "NOT_FOUND",
            "Run 'neomind device list' to see available devices",
        );
        assert!(!response.success);
        assert_eq!(response.error, Some("Device not found".to_string()));
        assert_eq!(
            response.suggestion,
            Some("Run 'neomind device list' to see available devices".to_string())
        );
    }

    #[test]
    fn test_cli_response_serialization() {
        let response = CliResponse::success(json!({"key": "value"}), "Success");

        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: CliResponse = serde_json::from_str(&serialized).unwrap();

        assert!(deserialized.success);
        assert_eq!(deserialized.message, Some("Success".to_string()));
        assert_eq!(deserialized.data, Some(json!({"key": "value"})));
    }

    #[test]
    fn test_output_format_equality() {
        assert_eq!(OutputFormat::Human, OutputFormat::Human);
        assert_eq!(OutputFormat::Json, OutputFormat::Json);
        assert_ne!(OutputFormat::Human, OutputFormat::Json);
    }
}
