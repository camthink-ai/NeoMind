//! `tools` — split from the former agent/mod.rs monolith.

use super::*;

use serde_json::Value;

use crate::error::Result;

impl Agent {
    fn sanitize_tool_output_for_llm(&self, tool_name: &str, data: &serde_json::Value) -> String {
        const MAX_SIZE: usize = 10_240; // 10KB
        const MAX_PREVIEW_ITEMS: usize = 5; // Keep first 5 array items
        const MAX_STRING_PREVIEW: usize = 500; // 500 chars for string preview

        // Special handling for device_discover - preserve critical structure
        if tool_name == "device_discover" {
            if let Some(obj) = data.as_object() {
                // Always preserve summary - it has accurate counts
                let summary = obj
                    .get("summary")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));

                // Extract minimal device info: id, name, type, status
                let devices: Vec<serde_json::Value> = obj.get("groups")
                    .and_then(|g| g.as_array())
                    .iter()
                    .flat_map(|groups| groups.iter())
                    .filter_map(|g| g.get("devices").and_then(|d| d.as_array()))
                    .flat_map(|devices| devices.iter())
                    .map(|device| {
                        serde_json::json!({
                            "id": device.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            "name": device.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "device_type": device.get("device_type").and_then(|v| v.as_str()).unwrap_or(""),
                            "status": device.get("status").and_then(|v| v.as_str()).unwrap_or("unknown")
                        })
                    })
                    .collect();

                let compact = serde_json::json!({
                    "summary": summary,
                    "device_count": devices.len(),
                    "devices": devices
                });

                let result = serde_json::to_string(&compact).unwrap_or_else(|_| "null".to_string());
                let original_size = serde_json::to_string(data).unwrap_or_default().len();

                if result.len() != original_size {
                    tracing::debug!(
                        session_id = %self.session_id,
                        tool = %tool_name,
                        original_bytes = original_size,
                        compressed_bytes = result.len(),
                        "device_discover output compressed for LLM"
                    );
                }

                return result;
            }
        }

        // Recursively truncate value to fit within MAX_SIZE
        let truncated = self.truncate_value(data, MAX_SIZE, MAX_PREVIEW_ITEMS, MAX_STRING_PREVIEW);

        let result = serde_json::to_string(&truncated).unwrap_or_else(|_| "null".to_string());
        let original_size = serde_json::to_string(data).unwrap_or_default().len();

        // Log if truncation occurred
        if result.len() != original_size {
            tracing::warn!(
                session_id = %self.session_id,
                tool = %tool_name,
                original_bytes = original_size,
                truncated_bytes = result.len(),
                "Tool output truncated for LLM"
            );
        }

        result
    }
}
impl Agent {
    /// Recursively truncate a JSON value to fit within size constraints.
    ///
    /// This preserves data structure while trimming large content.
    fn truncate_value(
        &self,
        value: &serde_json::Value,
        max_size: usize,
        max_items: usize,
        max_string: usize,
    ) -> serde_json::Value {
        match value {
            // Base64 detection: long string with only base64 chars
            serde_json::Value::String(s) if s.len() > 500 && self.is_likely_base64(s) => {
                // Replace base64 with metadata
                serde_json::json!({
                    "_truncated": true,
                    "_type": "base64_data",
                    "size_bytes": s.len()
                })
            }

            // Regular long string - truncate with preview
            serde_json::Value::String(s) if s.len() > max_string => {
                let preview: String = s.chars().take(max_string).collect();
                serde_json::json!({
                    "_truncated": true,
                    "_original_length": s.len(),
                    "preview": format!("{}...", preview)
                })
            }

            // Array - keep first N items + count
            serde_json::Value::Array(arr) => {
                if arr.len() <= max_items {
                    // Check each element recursively
                    let truncated: Vec<serde_json::Value> = arr
                        .iter()
                        .map(|v| {
                            self.truncate_value(
                                v,
                                max_size / arr.len().max(1),
                                max_items,
                                max_string,
                            )
                        })
                        .collect();
                    serde_json::Value::Array(truncated)
                } else {
                    let kept: Vec<serde_json::Value> = arr
                        .iter()
                        .take(max_items)
                        .map(|v| {
                            self.truncate_value(v, max_size / max_items, max_items, max_string)
                        })
                        .collect();

                    serde_json::json!({
                        "_truncated": true,
                        "_total_count": arr.len(),
                        "_showing_first": max_items,
                        "items": kept
                    })
                }
            }

            // Object - process each field, truncate large ones
            serde_json::Value::Object(obj) => {
                let mut result = serde_json::Map::new();

                for (key, val) in obj.iter() {
                    // Skip known large fields (like full data content)
                    if matches!(key.as_str(), "data" | "content" | "base64" | "image")
                        && serde_json::to_string(val).unwrap_or_default().len() > 1000
                    {
                        result.insert(
                            format!("_{}_truncated", key),
                            serde_json::json!({
                                "_size_bytes": serde_json::to_string(val).unwrap_or_default().len(),
                                "_note": "Large data omitted"
                            }),
                        );
                        continue;
                    }

                    let truncated = self.truncate_value(
                        val,
                        max_size / obj.len().max(1),
                        max_items,
                        max_string,
                    );
                    result.insert(key.clone(), truncated);

                    // Early exit if we're getting too big
                    if serde_json::to_string(&result).unwrap_or_default().len() > max_size {
                        break;
                    }
                }

                if !result.is_empty() {
                    serde_json::Value::Object(result)
                } else {
                    serde_json::json!({"_truncated": true, "_note": "All fields were too large"})
                }
            }

            // Other types pass through
            _ => value.clone(),
        }
    }
}
impl Agent {
    /// Check if a string is likely base64 encoded data.
    fn is_likely_base64(&self, s: &str) -> bool {
        // Base64 contains only these chars
        let base64_chars = s.chars().take(1000).all(|c| {
            c.is_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '\n' || c == '\r'
        });
        // Must be reasonably long and look like base64
        base64_chars && s.len() > 100
    }
}
impl Agent {
    /// Execute a tool with retry logic.
    ///
    /// Retries up to 2 times for transient errors (network issues, timeouts).
    /// Returns a user-friendly error message if all retries fail.
    ///
    /// ## Production-ready error context:
    /// - Includes tool name, arguments, session ID for traceability
    /// - Categorizes errors (transient, validation, execution, timeout)
    /// - Logs detailed error information for debugging
    pub(crate) async fn execute_tool(&self, name: &str, arguments: &Value) -> Result<String> {
        const MAX_RETRIES: u32 = 2;
        let start = std::time::Instant::now();

        // === CACHE: Check if result is cached ===
        let args_key = arguments.to_string();
        if let Some(cached_result) = self.tool_result_cache.read().await.get(name, &args_key) {
            let elapsed = start.elapsed();
            tracing::debug!(
                session_id = %self.session_id,
                tool = %name,
                elapsed_ms = elapsed.as_millis(),
                "Tool result served from cache"
            );
            return Ok(cached_result);
        }

        // Map simplified tool name to real tool name (for execution routing)
        let real_tool_name = self.resolve_tool_name(name);

        // Convert simplified parameter names to actual tool parameters.
        // Pass original name (not resolved) so domain-specific mapping works.
        let mapped_arguments = self.map_simplified_parameters(name, arguments);

        // === SEMANTIC MAPPING: Convert natural language to technical IDs ===
        // This maps "客厅灯" -> "light_living_main" for device_id parameters.
        // Use original name for domain matching (semantic_mapper expects "device", not "shell").
        let domain_name = crate::tools::mapper::resolve_domain_name(name);
        let semantically_mapped = self
            .semantic_mapper
            .map_tool_parameters(&domain_name, mapped_arguments.clone())
            .await
            .unwrap_or(mapped_arguments);

        // Sanitize arguments for logging (limit size to avoid log spam)
        let args_preview = if semantically_mapped.to_string().len() > 200 {
            format!(
                "{}...",
                semantically_mapped
                    .to_string()
                    .chars()
                    .take(200)
                    .collect::<String>()
            )
        } else {
            semantically_mapped.to_string()
        };

        tracing::debug!(
            session_id = %self.session_id,
            tool = %real_tool_name,
            arguments = %args_preview,
            "Executing tool"
        );

        // If mapper resolved a CLI domain name to "shell", convert structured args
        // to a CLI command string that ShellTool expects: {"command": "neomind <domain> ..."}
        let exec_args = if real_tool_name == "shell" && name != "shell" {
            crate::tools::mapper::build_cli_command(name, &semantically_mapped)
                .unwrap_or(semantically_mapped.clone())
        } else {
            semantically_mapped.clone()
        };

        let mut last_error = String::new();
        let mut last_attempt = 0u32;

        for attempt in 0..=MAX_RETRIES {
            match self.tools.execute(&real_tool_name, exec_args.clone()).await {
                Ok(output) => {
                    let elapsed = start.elapsed();

                    // Check if tool execution itself failed
                    if !output.success {
                        let error_msg = output.error.unwrap_or_else(|| "Unknown error".to_string());

                        // Log detailed error with context
                        tracing::error!(
                            session_id = %self.session_id,
                            tool = %real_tool_name,
                            arguments = %args_preview,
                            error = %error_msg,
                            attempt = attempt,
                            elapsed_ms = elapsed.as_millis(),
                            error_category = "tool_execution_failed",
                            "Tool execution returned failure"
                        );

                        // Don't retry on logical errors (like invalid input)
                        return Ok(format!(
                            "Tool {} execution failed: {}",
                            real_tool_name, error_msg
                        ));
                    }

                    tracing::debug!(
                        session_id = %self.session_id,
                        tool = %real_tool_name,
                        elapsed_ms = elapsed.as_millis(),
                        "Tool executed successfully"
                    );

                    // Sanitize output to avoid sending large data (base64, files, etc.) to LLM
                    let sanitized =
                        self.sanitize_tool_output_for_llm(&real_tool_name, &output.data);

                    // === CACHE: Handle write vs read operations ===
                    if is_write_action(arguments) {
                        // Invalidate all cached reads for this tool so subsequent
                        // queries reflect the updated state
                        self.tool_result_cache.write().await.invalidate(name);
                    } else {
                        // Cache read results to avoid redundant calls
                        self.tool_result_cache
                            .write()
                            .await
                            .put(name, args_key, sanitized.clone());
                    }

                    return Ok(sanitized);
                }
                Err(e) => {
                    last_error = e.to_string();
                    last_attempt = attempt;
                    let elapsed = start.elapsed();

                    // Categorize the error for better debugging
                    let error_category = if last_error.contains("not_found")
                        || last_error.contains("unknown")
                    {
                        "tool_not_found"
                    } else if last_error.contains("timeout") {
                        "timeout"
                    } else if last_error.contains("network") || last_error.contains("connection") {
                        "network"
                    } else if last_error.contains("parse") || last_error.contains("invalid") {
                        "validation"
                    } else {
                        "unknown"
                    };

                    // Check if error is transient (worth retrying)
                    let is_transient = matches!(error_category, "timeout" | "network");

                    tracing::warn!(
                        session_id = %self.session_id,
                        tool = %real_tool_name,
                        arguments = %args_preview,
                        error = %last_error,
                        attempt = attempt,
                        elapsed_ms = elapsed.as_millis(),
                        error_category = %error_category,
                        is_transient = is_transient,
                        "Tool execution error"
                    );

                    if is_transient && attempt < MAX_RETRIES {
                        // Exponential backoff: 100ms, 200ms
                        let delay_ms = 100 * (2_u64.pow(attempt));
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    }

                    // Non-transient errors: fail immediately, no retry
                    break;
                }
            }
        }

        // All retries failed - return detailed error with context
        let elapsed = start.elapsed();
        tracing::error!(
            session_id = %self.session_id,
            tool = %real_tool_name,
            arguments = %args_preview,
            elapsed_ms = elapsed.as_millis(),
            max_retries = MAX_RETRIES,
            error_category = "all_retries_failed",
            "Tool execution failed after all retries"
        );

        Err(crate::error::NeoMindError::Tool(format!(
            "Tool {} failed (session: {}, attempts: {}, elapsed: {}ms): {}",
            real_tool_name,
            self.session_id,
            last_attempt + 1,
            elapsed.as_millis(),
            last_error
        )))
    }
}
