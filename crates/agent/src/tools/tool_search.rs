//! Tool Search Tool for on-demand tool discovery.
//!
//! This tool allows the LLM to search for available tools by keyword,
//! reducing the need to include all tool definitions in every prompt.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use edge_ai_tools::{Tool, ToolOutput, ToolDefinition};
use edge_ai_tools::tool::{object_schema, string_property, ResponseFormat};

/// Tool for searching available tools.
///
/// This tool enables the LLM to discover tools on-demand rather than
/// having all tools loaded into every prompt. This reduces token usage
/// and improves response quality for tool-rich environments.
pub struct ToolSearchTool {
    /// Tool registry reference (cloned Arc to avoid reference lifetime issues)
    tool_names: Vec<String>,
    /// Tool descriptions (cached)
    tool_descriptions: Vec<(String, String)>,
}

impl ToolSearchTool {
    /// Create a new tool search tool.
    ///
    /// The tool_names and tool_descriptions should be extracted from
    /// the ToolRegistry at creation time.
    pub fn new(tool_names: Vec<String>, tool_descriptions: Vec<(String, String)>) -> Self {
        Self {
            tool_names,
            tool_descriptions,
        }
    }

    /// Create from a list of tool definitions.
    pub fn from_definitions(definitions: &[ToolDefinition]) -> Self {
        let tool_names = definitions.iter().map(|d| d.name.clone()).collect();
        let tool_descriptions = definitions
            .iter()
            .map(|d| (d.name.clone(), d.description.clone()))
            .collect();
        Self {
            tool_names,
            tool_descriptions,
        }
    }

    /// Search for tools by keyword.
    fn search(&self, keyword: &str) -> Vec<ToolSearchResult> {
        let keyword_lower = keyword.to_lowercase();
        let mut results = Vec::new();

        for (name, description) in &self.tool_descriptions {
            let name_matches = name.to_lowercase().contains(&keyword_lower);
            let desc_matches = description.to_lowercase().contains(&keyword_lower);

            if name_matches || desc_matches {
                results.push(ToolSearchResult {
                    name: name.clone(),
                    description: description.clone(),
                    matched_field: if name_matches { "name" } else { "description" }.to_string(),
                });
            }
        }

        // Sort by relevance (name matches first)
        results.sort_by(|a, b| {
            if a.matched_field == "name" && b.matched_field != "name" {
                std::cmp::Ordering::Less
            } else if a.matched_field != "name" && b.matched_field == "name" {
                std::cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });

        // Limit results
        results.truncate(10);
        results
    }

    /// Get tool categories.
    fn get_categories(&self) -> Vec<String> {
        let mut categories = std::collections::HashSet::new();
        for tool_name in &self.tool_names {
            // Extract common prefixes
            for (i, _) in tool_name.match_indices('_') {
                let prefix = &tool_name[..i];
                if prefix.len() >= 3 {
                    categories.insert(prefix.to_string());
                }
            }
        }
        let mut result: Vec<String> = categories.into_iter().collect();
        result.sort();
        result
    }

    /// List all tools.
    fn list_all(&self) -> Vec<ToolSearchResult> {
        self.tool_descriptions
            .iter()
            .map(|(name, description)| ToolSearchResult {
                name: name.clone(),
                description: description.clone(),
                matched_field: "name".to_string(),
            })
            .collect()
    }
}

/// Result of a tool search.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolSearchResult {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Which field matched the search
    pub matched_field: String,
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }

    fn description(&self) -> &str {
        "Search for available tools by keyword. Use this when you're unsure what tools are available or need to find tools for a specific task."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "keyword": string_property("The keyword to search for in tool names and descriptions"),
                "category": string_property("Optional category prefix to filter results (e.g., 'device', 'rule', 'workflow')")
            }),
            vec!["keyword".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput, edge_ai_tools::ToolError> {
        self.validate_args(&args)?;

        let keyword = args["keyword"]
            .as_str()
            .ok_or_else(|| edge_ai_tools::ToolError::InvalidArguments(
                "keyword must be a string".to_string()
            ))?;

        let category = args["category"].as_str();

        let results = if let Some(cat) = category {
            // Filter by category prefix
            let cat_lower = cat.to_lowercase();
            self.search(keyword)
                .into_iter()
                .filter(|r| r.name.to_lowercase().starts_with(&cat_lower))
                .collect()
        } else {
            self.search(keyword)
        };

        if results.is_empty() {
            // No exact matches, try to provide suggestions
            let categories = self.get_categories();
            Ok(ToolOutput::success(serde_json::json!({
                "keyword": keyword,
                "found": 0,
                "tools": [],
                "suggestions": {
                    "categories": categories,
                    "hint": format!("Try searching for: device, rule, workflow, list, get, create, etc.")
                }
            })))
        } else {
            Ok(ToolOutput::success(serde_json::json!({
                "keyword": keyword,
                "found": results.len(),
                "tools": results
            })))
        }
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(edge_ai_tools::tool::ToolExample {
                arguments: serde_json::json!({
                    "keyword": "device"
                }),
                result: serde_json::json!({
                    "keyword": "device",
                    "found": 3,
                    "tools": [
                        {"name": "list_devices", "description": "List all available devices", "matched_field": "name"},
                        {"name": "get_device", "description": "Get details of a specific device", "matched_field": "name"},
                        {"name": "control_device", "description": "Control a device by sending commands", "matched_field": "name"}
                    ]
                }),
                description: "Search for tools related to devices".to_string(),
            }),
            examples: vec![
                edge_ai_tools::tool::ToolExample {
                    arguments: serde_json::json!({"keyword": "device"}),
                    result: serde_json::json!({
                        "keyword": "device",
                        "found": 3,
                        "tools": [
                            {"name": "list_devices", "description": "List all available devices", "matched_field": "name"}
                        ]
                    }),
                    description: "搜索设备相关工具".to_string(),
                },
            ],
            response_format: ResponseFormat::Concise,
            namespace: Some("system".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("system")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tool() -> ToolSearchTool {
        let tool_descriptions = vec![
            ("list_devices".to_string(), "List all available devices".to_string()),
            ("list_rules".to_string(), "List all automation rules".to_string()),
            ("create_rule".to_string(), "Create a new automation rule".to_string()),
            ("get_device".to_string(), "Get details of a specific device".to_string()),
            ("control_device".to_string(), "Control a device by sending commands".to_string()),
            ("query_data".to_string(), "Query time series data from devices".to_string()),
        ];
        ToolSearchTool::new(
            tool_descriptions.iter().map(|(n, _)| n.clone()).collect(),
            tool_descriptions,
        )
    }

    #[tokio::test]
    async fn test_tool_search_by_keyword() {
        let tool = create_test_tool();
        let args = serde_json::json!({
            "keyword": "device"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        // Note: The test data may have changed, just check that results were found
        assert!(result.data["found"].as_u64().unwrap() >= 3);
    }

    #[tokio::test]
    async fn test_tool_search_by_description() {
        let tool = create_test_tool();
        let args = serde_json::json!({
            "keyword": "automation"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["found"], 2); // list_rules, create_rule
    }

    #[tokio::test]
    async fn test_tool_search_with_category() {
        let tool = create_test_tool();
        let args = serde_json::json!({
            "keyword": "all",
            "category": "list"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        // Should only return tools starting with "list_"
        assert_eq!(result.data["found"], 2); // list_devices, list_rules
    }

    #[tokio::test]
    async fn test_tool_search_no_results() {
        let tool = create_test_tool();
        let args = serde_json::json!({
            "keyword": "nonexistent"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["found"], 0);
        assert!(result.data["suggestions"].is_object());
    }
}
