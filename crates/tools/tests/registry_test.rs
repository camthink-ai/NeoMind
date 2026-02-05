//! Comprehensive tests for the Tool Registry system.
//!
//! Tests include:
//! - Tool registration
//! - Tool discovery
//! - Tool execution
//! - Parallel tool execution
//! - Error handling

use edge_ai_tools::{Tool, ToolRegistry, ToolRegistryBuilder, ToolOutput};
use edge_ai_tools::{object_schema, number_property, string_property};
use edge_ai_tools::ToolCall;
use serde_json::json;
use std::sync::Arc;

// Simple test tool
struct TestTool {
    name: String,
}

#[async_trait::async_trait]
impl Tool for TestTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "A test tool"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "value": string_property("A test value")
            }),
            vec!["value".to_string()]
        )
    }

    async fn execute(&self, args: serde_json::Value) -> edge_ai_tools::Result<ToolOutput> {
        let value = args["value"]
            .as_str()
            .unwrap_or("default");

        Ok(ToolOutput::success(json!({
            "result": format!("Processed: {}", value)
        })))
    }
}

// Another test tool
struct AddTool;

#[async_trait::async_trait]
impl Tool for AddTool {
    fn name(&self) -> &str {
        "add"
    }

    fn description(&self) -> &str {
        "Add two numbers"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "a": number_property("First number"),
                "b": number_property("Second number")
            }),
            vec!["a".to_string(), "b".to_string()]
        )
    }

    async fn execute(&self, args: serde_json::Value) -> edge_ai_tools::Result<ToolOutput> {
        let a: f64 = args["a"].as_f64().unwrap_or(0.0);
        let b: f64 = args["b"].as_f64().unwrap_or(0.0);

        Ok(ToolOutput::success(json!({
            "result": a + b
        })))
    }
}

#[tokio::test]
async fn test_registry_new() {
    let registry = ToolRegistry::new();

    // New registry should be empty
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
}

#[tokio::test]
async fn test_registry_register_tool() {
    let mut registry = ToolRegistry::new();

    let tool = Arc::new(TestTool {
        name: "test_tool".to_string(),
    });

    registry.register(tool);
    assert_eq!(registry.len(), 1);
    assert!(registry.has("test_tool"));
}

#[tokio::test]
async fn test_registry_get_tool() {
    let mut registry = ToolRegistry::new();

    let tool = Arc::new(TestTool {
        name: "my_tool".to_string(),
    });

    registry.register(tool.clone());

    let retrieved = registry.get("my_tool");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name(), "my_tool");
}

#[tokio::test]
async fn test_registry_execute_tool() {
    let mut registry = ToolRegistry::new();

    let add_tool = Arc::new(AddTool);
    registry.register(add_tool);

    let result = registry
        .execute("add", json!({"a": 5, "b": 3}))
        .await;

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.success);
    assert_eq!(output.data["result"], 8.0);
}

#[tokio::test]
async fn test_registry_execute_missing_parameter() {
    let mut registry = ToolRegistry::new();

    let add_tool = Arc::new(AddTool);
    registry.register(add_tool);

    let result = registry.execute("add", json!({"a": 5})).await;

    // Should still execute but with default value
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_registry_execute_nonexistent_tool() {
    let registry = ToolRegistry::new();

    let result = registry
        .execute("nonexistent_tool", json!({}))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_registry_has_tool() {
    let mut registry = ToolRegistry::new();

    assert!(!registry.has("test_tool"));

    let tool = Arc::new(TestTool {
        name: "test_tool".to_string(),
    });
    registry.register(tool);

    assert!(registry.has("test_tool"));
}

#[tokio::test]
async fn test_registry_list_tools() {
    let mut registry = ToolRegistry::new();

    // Register multiple tools
    registry.register(Arc::new(TestTool {
        name: "tool1".to_string(),
    }));
    registry.register(Arc::new(TestTool {
        name: "tool2".to_string(),
    }));
    registry.register(Arc::new(AddTool));

    assert_eq!(registry.len(), 3);
    assert!(registry.has("tool1"));
    assert!(registry.has("tool2"));
    assert!(registry.has("add"));
}

#[tokio::test]
async fn test_tool_output_success() {
    let output = ToolOutput::success(json!({"result": "ok"}));

    assert!(output.success);
    assert!(!output.error.is_some());
    assert_eq!(output.data["result"], "ok");
}

#[tokio::test]
async fn test_tool_output_error() {
    let output = ToolOutput::error("Something went wrong");

    assert!(!output.success);
    assert!(output.error.is_some());
    assert_eq!(output.error, Some("Something went wrong".to_string()));
}

#[tokio::test]
async fn test_parallel_execution() {
    let mut registry = ToolRegistry::new();

    registry.register(Arc::new(AddTool));

    let calls = vec![
        ToolCall::new("add", json!({"a": 1, "b": 2})),
        ToolCall::new("add", json!({"a": 10, "b": 20})),
        ToolCall::new("add", json!({"a": 100, "b": 200})),
    ];

    let results = registry.execute_parallel(calls).await;

    assert_eq!(results.len(), 3);
    assert!(results[0].result.as_ref().unwrap().success);
    assert_eq!(results[0].result.as_ref().unwrap().data["result"], 3.0);
    assert_eq!(results[1].result.as_ref().unwrap().data["result"], 30.0);
    assert_eq!(results[2].result.as_ref().unwrap().data["result"], 300.0);
}

#[tokio::test]
async fn test_parallel_execution_with_errors() {
    let mut registry = ToolRegistry::new();

    registry.register(Arc::new(AddTool));

    let calls = vec![
        ToolCall::new("add", json!({"a": 1, "b": 2})),
        ToolCall::new("add", json!({"a": 10})), // Missing 'b'
        ToolCall::new("add", json!({"a": 100, "b": 200})),
    ];

    let results = registry.execute_parallel(calls).await;

    assert_eq!(results.len(), 3);
    assert!(results[0].result.as_ref().unwrap().success);
    // Second one still works (with default value)
    assert!(results[1].result.as_ref().unwrap().success);
    assert!(results[2].result.as_ref().unwrap().success);
}

#[tokio::test]
async fn test_tool_definition() {
    let tool = TestTool {
        name: "my_tool".to_string(),
    };

    let definition = tool.definition();

    assert_eq!(definition.name, "my_tool");
    assert_eq!(definition.description, "A test tool");
}

#[tokio::test]
async fn test_registry_unregister() {
    let mut registry = ToolRegistry::new();

    let tool = Arc::new(TestTool {
        name: "temp_tool".to_string(),
    });
    registry.register(tool);

    assert!(registry.has("temp_tool"));

    registry.unregister("temp_tool");

    assert!(!registry.has("temp_tool"));
}

#[tokio::test]
async fn test_tool_execution_with_complex_args() {
    let mut registry = ToolRegistry::new();

    registry.register(Arc::new(TestTool {
        name: "complex_tool".to_string(),
    }));

    let args = json!({
        "value": "test with spaces",
        "extra": "ignored"
    });

    let result = registry.execute("complex_tool", args).await;

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.success);
    assert!(output.data["result"].as_str().unwrap().contains("test with spaces"));
}

#[tokio::test]
async fn test_multiple_registries() {
    let mut registry1 = ToolRegistry::new();
    let mut registry2 = ToolRegistry::new();

    registry1.register(Arc::new(TestTool {
        name: "tool1".to_string(),
    }));
    registry2.register(Arc::new(TestTool {
        name: "tool2".to_string(),
    }));

    assert!(registry1.has("tool1"));
    assert!(!registry1.has("tool2"));
    assert!(registry2.has("tool2"));
    assert!(!registry2.has("tool1"));
}

#[tokio::test]
async fn test_empty_args() {
    let mut registry = ToolRegistry::new();

    registry.register(Arc::new(TestTool {
        name: "requires_value".to_string(),
    }));

    let result = registry.execute("requires_value", json!({})).await;

    // Should still work with default value
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.success);
}

#[tokio::test]
async fn test_registry_builder() {
    let registry = ToolRegistryBuilder::new()
        .with_tool(Arc::new(TestTool {
            name: "test".to_string(),
        }))
        .build();

    assert!(registry.has("test"));
    assert_eq!(registry.len(), 1);
}

#[tokio::test]
async fn test_format_for_llm() {
    let mut registry = ToolRegistry::new();

    registry.register(Arc::new(AddTool));
    registry.register(Arc::new(TestTool {
        name: "test".to_string(),
    }));

    let definitions = registry.definitions();
    let formatted = edge_ai_tools::format_for_llm(&definitions);

    assert!(formatted.contains("add"));
    assert!(formatted.contains("test"));
}

#[tokio::test]
async fn test_registry_search() {
    let mut registry = ToolRegistry::new();

    registry.register(Arc::new(TestTool {
        name: "temperature_tool".to_string(),
    }));
    registry.register(Arc::new(TestTool {
        name: "humidity_tool".to_string(),
    }));

    // Search for temperature tools
    let results = registry.search("temperature");
    assert!(results.len() > 0);
    assert!(results[0].name.contains("temperature"));
}

#[tokio::test]
async fn test_registry_list() {
    let mut registry = ToolRegistry::new();

    registry.register(Arc::new(TestTool {
        name: "tool1".to_string(),
    }));
    registry.register(Arc::new(TestTool {
        name: "tool2".to_string(),
    }));

    let tools = registry.list();
    assert_eq!(tools.len(), 2);
    assert!(tools.contains(&"tool1".to_string()));
    assert!(tools.contains(&"tool2".to_string()));
}

#[tokio::test]
async fn test_registry_definitions() {
    let mut registry = ToolRegistry::new();

    registry.register(Arc::new(AddTool));

    let definitions = registry.definitions();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].name, "add");
}
