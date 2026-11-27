//! API-level tools that require access to API-layer dependencies.
//!
//! These tools are defined here because they depend on types from neomind-api
//! (like AutomationStore) which cannot be imported into neomind-agent due to
//! circular dependency issues.

use crate::automation::store::SharedAutomationStore;
use crate::automation::transform::JsTransformExecutor;
use crate::automation::types::{
    Automation, AutomationMetadata, TransformAutomation, TransformScope,
};
use async_trait::async_trait;
use neomind_agent::toolkit::time_utils::TransformStore;
use neomind_agent::toolkit::{
    object_schema, string_property, Tool, ToolCategory, ToolDefinition, ToolError, ToolExample,
    ToolOutput, ToolRelationships, UsageScenario,
};
use serde_json::Value;

// ============================================================================
// TransformStore trait impl for SharedAutomationStore
// ============================================================================

#[async_trait]
impl TransformStore for SharedAutomationStore {
    async fn save_transform(&self, data: Value) -> std::result::Result<String, String> {
        // The tool sends a custom JSON format. We need to build Automation manually
        // because the tool's JSON doesn't match the serde format of the Automation type.
        //
        // Tool sends: { "metadata": { "id": ..., ... }, "scope": "global", ... }
        // Or for create: data is the inner transform fields directly
        // Or for update: data is the existing TransformAutomation with merged fields
        //
        // We try to extract the inner transform data from possible envelope shapes:
        let inner = if data.is_object() && data.get("transform").is_some() {
            // Wrapped: {"transform": {...}}
            data.get("transform")
                .cloned()
                .expect("transform field existence verified above")
        } else if data.is_object() && data.get("type").is_some() {
            // Already an Automation serde format: {"type": "transform", ...}
            // Note: Automation is now a type alias for TransformAutomation, but serde may still
            // expect the {"type": "transform", ...} envelope if the data was serialized that way.
            let automation: Automation = serde_json::from_value(data)
                .map_err(|e| format!("Invalid transform data: {}", e))?;
            let id = automation.id().to_string();
            self.save_automation(&automation)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(id);
        } else {
            // Direct: the fields are at the top level
            data.clone()
        };

        // Build TransformAutomation from the inner JSON
        // Fields can be either nested under "metadata" (create format) or flat (update format)
        let metadata_obj = inner.get("metadata");
        let get_str = |key: &str| -> Option<String> {
            // Try metadata nested first, then flat
            metadata_obj
                .and_then(|m| m.get(key))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    inner
                        .get(key)
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
        };
        let get_bool = |key: &str| -> Option<bool> {
            metadata_obj
                .and_then(|m| m.get(key))
                .and_then(|v| v.as_bool())
                .or_else(|| inner.get(key).and_then(|v| v.as_bool()))
        };
        let get_u64 = |key: &str| -> Option<u64> {
            metadata_obj
                .and_then(|m| m.get(key))
                .and_then(|v| v.as_u64())
                .or_else(|| inner.get(key).and_then(|v| v.as_u64()))
        };
        let get_i64 = |key: &str| -> Option<i64> {
            metadata_obj
                .and_then(|m| m.get(key))
                .and_then(|v| v.as_i64())
                .or_else(|| inner.get(key).and_then(|v| v.as_i64()))
        };

        let id = get_str("id").unwrap_or_default();
        let name = get_str("name").unwrap_or_else(|| "Unnamed Transform".to_string());
        let description = get_str("description").unwrap_or_default();
        let enabled = get_bool("enabled").unwrap_or(true);
        let execution_count = get_u64("execution_count").unwrap_or(0);
        let last_executed = get_i64("last_executed");
        let created_at = get_i64("created_at").unwrap_or_else(|| chrono::Utc::now().timestamp());
        let updated_at = get_i64("updated_at").unwrap_or_else(|| chrono::Utc::now().timestamp());

        let scope_str = inner
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("global");
        let scope = parse_scope(scope_str).map_err(|e| e.to_string())?;

        let intent = inner
            .get("intent")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let js_code = inner
            .get("js_code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let output_prefix = inner
            .get("output_prefix")
            .and_then(|v| v.as_str())
            .unwrap_or("transform")
            .to_string();
        let complexity = inner
            .get("complexity")
            .and_then(|v| v.as_u64())
            .unwrap_or(2) as u8;

        let mut meta = AutomationMetadata::new(&id, &name).with_description(description);
        meta.enabled = enabled;
        meta.execution_count = execution_count;
        meta.last_executed = last_executed;
        meta.created_at = created_at;
        meta.updated_at = updated_at;

        let transform = TransformAutomation {
            metadata: meta,
            scope,
            intent,
            js_code,
            output_prefix,
            complexity,
            operations: None,
        };

        let automation = transform;
        self.save_automation(&automation)
            .await
            .map_err(|e| e.to_string())?;
        Ok(id)
    }

    async fn get_transform(&self, id: &str) -> std::result::Result<Option<Value>, String> {
        match self.get_automation(id).await.map_err(|e| e.to_string())? {
            Some(t) => {
                let mut val = serde_json::to_value(&t).map_err(|e| e.to_string())?;
                // Replace enum scope with human-readable string
                val["scope"] = Value::String(t.scope.as_str());
                Ok(Some(val))
            }
            None => Ok(None),
        }
    }

    async fn list_transforms(&self) -> std::result::Result<Vec<Value>, String> {
        let all = self.list_automations().await.map_err(|e| e.to_string())?;
        Ok(all
            .into_iter()
            .map(|t| {
                let mut val = serde_json::to_value(&t).unwrap_or_default();
                // Replace enum scope with human-readable string
                val["scope"] = Value::String(t.scope.as_str());
                val
            })
            .collect())
    }

    async fn delete_transform(&self, id: &str) -> std::result::Result<bool, String> {
        match self.get_automation(id).await.map_err(|e| e.to_string())? {
            Some(_) => self.delete_automation(id).await.map_err(|e| e.to_string()),
            None => Ok(false),
        }
    }
}

// ============================================================================
// Transform Tool - Data transformation rules
// ============================================================================

/// Transform tool for managing data transformation rules.
///
/// Transforms use JavaScript to process raw device data into standardized metrics.
/// They are stored in the AutomationStore and can be applied to devices based on scope.
pub struct TransformTool {
    automation_store: SharedAutomationStore,
}

impl TransformTool {
    /// Create a new transform tool.
    pub fn new(automation_store: SharedAutomationStore) -> Self {
        Self { automation_store }
    }
}

#[async_trait]
impl Tool for TransformTool {
    fn name(&self) -> &str {
        "transform"
    }

    fn description(&self) -> &str {
        "数据转换规则工具。action: list(列出转换规则), get(详情), create(创建), update(更新), delete(删除), test(测试转换)"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "action": {
                    "type": "string",
                    "enum": ["list", "get", "create", "update", "delete", "test"],
                    "description": "操作类型"
                },
                "transform_id": string_property("转换规则ID (get/update/delete/test必填)"),
                "name": string_property("规则名称 (create/update必填)"),
                "description": string_property("规则描述 (create/update可选)"),
                "scope": string_property("作用域: global(全局), device_type:类型名, device:设备ID (create/update必填)"),
                "intent": string_property("自然语言描述转换意图 (create/update可选，AI可据此生成代码)"),
                "js_code": string_property("JavaScript转换代码 (create/update可选，如不提供则根据intent生成)"),
                "output_prefix": string_property("输出指标前缀 (create/update可选，默认为'transform')"),
                "input_data": {
                    "type": "object",
                    "description": "测试输入数据 (test必填)"
                },
                "limit": {
                    "type": "number",
                    "description": "返回数量限制 (list可选)"
                }
            }),
            vec!["action".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Rule
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "action": "list"
                }),
                result: serde_json::json!({
                    "count": 2,
                    "transforms": [
                        {"id": "transform_1", "name": "YOLO检测统计", "scope": "global"},
                        {"id": "transform_2", "name": "温度数据转换", "scope": "device_type:temperature_sensor"}
                    ]
                }),
                description: "列出所有转换规则".to_string(),
            }),
            category: ToolCategory::Rule,
            scenarios: vec![
                UsageScenario {
                    description: "查看数据转换规则".to_string(),
                    example_query: "有哪些数据转换规则？".to_string(),
                    suggested_call: Some(
                        r#"{"tool": "transform", "arguments": {"action": "list"}}"#.to_string(),
                    ),
                },
                UsageScenario {
                    description: "创建数据转换规则".to_string(),
                    example_query: "创建一个统计detections数量的转换规则".to_string(),
                    suggested_call: Some(
                        r#"{"tool": "transform", "arguments": {"action": "create", "name": "检测统计", "scope": "global", "intent": "统计detections数组中每个类别的数量"}}"#
                            .to_string(),
                    ),
                },
            ],
            relationships: ToolRelationships::default(),
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("automation".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("automation")
    }

    async fn execute(&self, args: Value) -> neomind_agent::toolkit::Result<ToolOutput> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("action is required".into()))?;

        match action {
            "list" => self.execute_list(&args).await,
            "get" => self.execute_get(&args).await,
            "create" => self.execute_create(&args).await,
            "update" => self.execute_update(&args).await,
            "delete" => self.execute_delete(&args).await,
            "test" => self.execute_test(&args).await,
            _ => Err(ToolError::InvalidArguments(format!(
                "Unknown action: {}",
                action
            ))),
        }
    }
}

impl TransformTool {
    async fn execute_list(&self, args: &Value) -> neomind_agent::toolkit::Result<ToolOutput> {
        let limit = args["limit"].as_u64().unwrap_or(100) as usize;

        let automations = self
            .automation_store
            .list_automations()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let transforms: Vec<Value> = automations
            .into_iter()
            .take(limit)
            .map(|t| {
                serde_json::json!({
                    "id": t.metadata.id,
                    "name": t.metadata.name,
                    "description": t.metadata.description,
                    "scope": t.scope.as_str(),
                    "enabled": t.metadata.enabled,
                    "execution_count": t.metadata.execution_count
                })
            })
            .collect();

        Ok(ToolOutput::success(serde_json::json!({
            "count": transforms.len(),
            "transforms": transforms
        })))
    }

    async fn execute_get(&self, args: &Value) -> neomind_agent::toolkit::Result<ToolOutput> {
        let transform_id = args["transform_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("transform_id is required".into()))?;

        let automation = self
            .automation_store
            .get_automation(transform_id)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        match automation {
            Some(t) => Ok(ToolOutput::success(serde_json::json!({
                "id": t.metadata.id,
                "name": t.metadata.name,
                "description": t.metadata.description,
                "scope": t.scope.as_str(),
                "enabled": t.metadata.enabled,
                "intent": t.intent,
                "js_code": t.js_code,
                "output_prefix": t.output_prefix,
                "execution_count": t.metadata.execution_count,
                "last_executed": t.metadata.last_executed
            }))),
            None => Err(ToolError::Execution(format!(
                "Transform not found: {}",
                transform_id
            ))),
        }
    }

    async fn execute_create(&self, args: &Value) -> neomind_agent::toolkit::Result<ToolOutput> {
        let name = args["name"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("name is required".into()))?;

        let scope_str = args["scope"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("scope is required".into()))?;

        let scope = parse_scope(scope_str)?;

        let intent = args["intent"].as_str().map(|s| s.to_string());
        let js_code = args["js_code"].as_str().map(|s| s.to_string());
        let description = args["description"].as_str().unwrap_or("").to_string();
        let output_prefix = args["output_prefix"]
            .as_str()
            .unwrap_or("transform")
            .to_string();

        let id = format!("transform_{}", uuid::Uuid::new_v4());

        let metadata = AutomationMetadata::new(&id, name).with_description(description);

        let transform = TransformAutomation {
            metadata,
            scope,
            intent,
            js_code,
            output_prefix,
            complexity: 2,
            operations: None,
        };

        let automation = transform;

        self.automation_store
            .save_automation(&automation)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(ToolOutput::success(serde_json::json!({
            "id": id,
            "status": "created",
            "message": "转换规则创建成功"
        })))
    }

    async fn execute_update(&self, args: &Value) -> neomind_agent::toolkit::Result<ToolOutput> {
        let transform_id = args["transform_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("transform_id is required".into()))?;

        // Get the existing transform
        let automation = self
            .automation_store
            .get_automation(transform_id)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        match automation {
            Some(mut t) => {
                // Update fields if provided
                if let Some(name) = args["name"].as_str() {
                    t.metadata.name = name.to_string();
                }
                if let Some(description) = args["description"].as_str() {
                    t.metadata.description = description.to_string();
                }
                if let Some(scope_str) = args["scope"].as_str() {
                    t.scope = parse_scope(scope_str)?;
                }
                if args.get("intent").is_some() {
                    t.intent = args["intent"].as_str().map(|s| s.to_string());
                }
                if args.get("js_code").is_some() {
                    t.js_code = args["js_code"].as_str().map(|s| s.to_string());
                }
                if let Some(output_prefix) = args["output_prefix"].as_str() {
                    t.output_prefix = output_prefix.to_string();
                }

                self.automation_store
                    .save_automation(&t)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;

                Ok(ToolOutput::success(serde_json::json!({
                    "id": transform_id,
                    "status": "updated",
                    "message": "转换规则更新成功"
                })))
            }
            None => Err(ToolError::Execution(format!(
                "Transform not found: {}",
                transform_id
            ))),
        }
    }

    async fn execute_delete(&self, args: &Value) -> neomind_agent::toolkit::Result<ToolOutput> {
        let transform_id = args["transform_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("transform_id is required".into()))?;

        // Verify it's a transform
        let automation = self
            .automation_store
            .get_automation(transform_id)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        match automation {
            Some(_) => {
                self.automation_store
                    .delete_automation(transform_id)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;

                Ok(ToolOutput::success(serde_json::json!({
                    "id": transform_id,
                    "status": "deleted"
                })))
            }
            None => Err(ToolError::Execution(format!(
                "Transform not found: {}",
                transform_id
            ))),
        }
    }

    async fn execute_test(&self, args: &Value) -> neomind_agent::toolkit::Result<ToolOutput> {
        let transform_id = args["transform_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("transform_id is required".into()))?;

        let input_data = args["input_data"].clone();

        // Get the transform
        let automation = self
            .automation_store
            .get_automation(transform_id)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        match automation {
            Some(t) => {
                let js_code = t.js_code.ok_or_else(|| {
                    ToolError::Execution("Transform has no JavaScript code".into())
                })?;

                // Execute the transform using the JsTransformExecutor
                let result = JsTransformExecutor::execute_async(
                    js_code,
                    input_data.clone(),
                    "test".to_string(),
                    "test_device".to_string(),
                    chrono::Utc::now().timestamp(),
                    None,
                    // Chat-side code preview: dry-run — no real
                    // extension invocations from a test execution.
                    crate::automation::transform::TransformExecutionMode::DryRun,
                )
                .await
                .map_err(|e| ToolError::Execution(format!("Transform execution failed: {}", e)))?;

                Ok(ToolOutput::success(serde_json::json!({
                    "input": input_data,
                    "output": result.metrics,
                    "metrics_count": result.metrics.len(),
                    "unverified": if result.extension_invoke_blocked {
                        serde_json::json!([
                            "extension invocation blocked in dry-run; extension-dependent behavior unverified"
                        ])
                    } else {
                        serde_json::json!([])
                    },
                })))
            }
            None => Err(ToolError::Execution(format!(
                "Transform not found: {}",
                transform_id
            ))),
        }
    }
}

/// Parse scope string into TransformScope
fn parse_scope(scope_str: &str) -> neomind_agent::toolkit::Result<TransformScope> {
    if scope_str == "global" {
        Ok(TransformScope::Global)
    } else if let Some(device_type) = scope_str.strip_prefix("device_type:") {
        Ok(TransformScope::DeviceType(device_type.to_string()))
    } else if let Some(device_id) = scope_str.strip_prefix("device:") {
        Ok(TransformScope::Device(device_id.to_string()))
    } else {
        Err(ToolError::InvalidArguments(
            "Invalid scope. Use: global, device_type:xxx, or device:xxx".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_scope() {
        assert!(matches!(parse_scope("global"), Ok(TransformScope::Global)));
        assert!(matches!(
            parse_scope("device_type:temperature"),
            Ok(TransformScope::DeviceType(_))
        ));
        assert!(matches!(
            parse_scope("device:sensor_1"),
            Ok(TransformScope::Device(_))
        ));
        assert!(parse_scope("invalid").is_err());
    }
}
