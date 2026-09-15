use super::*;

fn record_notification(
    notifications: &mut Vec<neomind_storage::NotificationSent>,
    channel: &str,
    recipient: &str,
    message: String,
    success: bool,
) {
    notifications.push(neomind_storage::NotificationSent {
        channel: channel.to_string(),
        recipient: recipient.to_string(),
        message,
        sent_at: chrono::Utc::now().timestamp(),
        success,
    });
}

impl AgentExecutor {
    pub(crate) async fn execute_single_command(
        &self,
        agent: &AiAgent,
        resource: &AgentResource,
        decision: &Decision,
    ) -> Option<neomind_storage::ActionExecuted> {
        let device_service = self.device_service.as_ref()?;

        // Parse device_id and command from resource_id (format: "device_id:command_name")
        let parts: Vec<&str> = resource.resource_id.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        let device_id = parts[0];
        let command_name = parts[1];

        let parameters = resource
            .config
            .get("parameters")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        // Clone parameters for DeviceService (will be consumed)
        let params_map: std::collections::HashMap<String, serde_json::Value> = parameters
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        tracing::info!(
            agent_id = %agent.id,
            device_id = %device_id,
            command = %command_name,
            decision_action = %decision.action,
            "Executing command from LLM decision"
        );

        // Execute the command via DeviceService
        let execution_result = device_service
            .send_command(device_id, command_name, params_map)
            .await;

        let (success, result) = match execution_result {
            Ok(_) => (true, Some("Command sent successfully".to_string())),
            Err(e) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    device_id = %device_id,
                    command = %command_name,
                    error = %e,
                    "Failed to send command"
                );
                (false, Some(format!("Failed: {}", e)))
            }
        };

        Some(neomind_storage::ActionExecuted {
            action_type: "device_command".to_string(),
            description: format!(
                "Execute {} on {} (reason: {})",
                command_name, device_id, decision.rationale
            ),
            target: device_id.to_string(),
            parameters: serde_json::to_value(&parameters).unwrap_or_default(),
            success,
            result,
        })
    }

    pub(crate) async fn execute_extension_command_for_agent(
        &self,
        agent: &AiAgent,
        extension_id: &str,
        command_name: &str,
        decision: &Decision,
    ) -> Option<neomind_storage::ActionExecuted> {
        let extension_registry = self.extension_registry.as_ref()?;

        tracing::info!(
            agent_id = %agent.id,
            extension_id = %extension_id,
            command = %command_name,
            decision_action = %decision.action,
            "Executing extension command from LLM decision"
        );

        // Build parameters from resource config or decision
        let command_args = decision.rationale.clone();
        let args_value = if command_args.is_empty() {
            serde_json::json!({})
        } else {
            // Try to parse as JSON, otherwise wrap as string
            serde_json::from_str(&command_args)
                .unwrap_or_else(|_| serde_json::json!({ "reason": command_args }))
        };

        // Execute the extension command
        let execution_result = extension_registry
            .execute_command(extension_id, command_name, &args_value)
            .await;

        let (success, result) = match execution_result {
            Ok(resp) => (true, Some(format!("Success: {}", resp))),
            Err(e) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    extension_id = %extension_id,
                    command = %command_name,
                    error = %e,
                    "Failed to execute extension command"
                );
                (false, Some(format!("Failed: {}", e)))
            }
        };

        Some(neomind_storage::ActionExecuted {
            action_type: "extension_command".to_string(),
            description: format!(
                "Execute {} on extension {} (reason: {})",
                command_name, extension_id, decision.rationale
            ),
            target: extension_id.to_string(),
            parameters: args_value,
            success,
            result,
        })
    }

    /// Parse command from decision.action field.
    ///
    /// Expected formats:
    /// - "device_id:command_name" -> device command
    /// - "extension:ext_id:command_name" -> extension command
    pub(crate) fn parse_command_from_action(action: &str) -> Option<(String, String, String)> {
        let action = action.trim();

        // Try to parse as "prefix:id:command_name"
        if let Some(colon_pos) = action.find(':') {
            let prefix = &action[..colon_pos];
            let rest = &action[colon_pos + 1..];

            // Check if it's "extension:ext_id:command_name"
            if prefix == "extension" || prefix == "ext" {
                if let Some(second_colon) = rest.find(':') {
                    let ext_id = &rest[..second_colon];
                    let command_name = &rest[second_colon + 1..];
                    if !ext_id.is_empty() && !command_name.is_empty() {
                        return Some((
                            "extension".to_string(),
                            ext_id.trim().to_string(),
                            command_name.trim().to_string(),
                        ));
                    }
                }
            }

            // Otherwise treat as "device_id:command_name"
            if !prefix.is_empty() && !rest.is_empty() {
                return Some((
                    "device".to_string(),
                    prefix.trim().to_string(),
                    rest.trim().to_string(),
                ));
            }
        }

        // Try to parse as "device:command" (common format)
        if action.contains("device:") || action.contains("设备:") {
            // Extract device:command pattern using regex-like parsing
            let parts: Vec<&str> = action.split([':', '：']).map(|s| s.trim()).collect();
            if parts.len() >= 3 {
                // Format: "device:xxx:command" or similar
                let cmd_keyword_idx = parts
                    .iter()
                    .position(|&p| p == "device" || p == "设备" || p == "command" || p == "指令");
                if let Some(idx) = cmd_keyword_idx {
                    if idx + 1 < parts.len() {
                        return Some((
                            "device".to_string(),
                            parts[idx + 1].to_string(),
                            parts[idx + 2].to_string(),
                        ));
                    }
                }
            }
        }

        None
    }

    fn is_alert_decision(decision: &Decision) -> bool {
        let dt = decision.decision_type.to_lowercase();
        let da = decision.action.to_lowercase();

        // Tool execution results are NOT alert decisions — they're just query/control results
        // from the tool-calling loop. Only actual LLM analysis decisions should trigger alerts.
        if dt == "tool_execution" {
            return false;
        }

        dt.contains("alert")
            || da.contains("alert")
            || da.contains("报警")
            || da.contains("notify")
            || da.contains("通知")
    }

    fn is_execute_action(action: &str) -> bool {
        let a = action.to_lowercase();
        a.contains("execute_command")
            || a.contains("command")
            || a.contains("执行指令")
            || a.contains("控制")
    }

    async fn handle_query_decision(
        &self,
        agent: &AiAgent,
        decision: &Decision,
        actions: &mut Vec<neomind_storage::ActionExecuted>,
    ) {
        let parts: Vec<&str> = decision.action.split(':').collect();
        if parts.len() < 4 {
            return;
        }
        let device_id = parts[1];
        let metric = parts[2];
        let time_spec = parts[3];

        tracing::info!(
            agent_id = %agent.id,
            device_id = %device_id,
            metric = %metric,
            time_spec = %time_spec,
            decision_action = %decision.action,
            "Agent requested data with specific time range"
        );

        // Execute the actual query if time_series_storage is available
        let query_result = if let Some(storage) = &self.time_series_storage {
            let default_secs = 24 * 3600; // 24h
            let range_secs =
                crate::toolkit::time_utils::parse_time_range(time_spec).unwrap_or(default_secs);
            let end_time = chrono::Utc::now().timestamp();
            let start_time = end_time - range_secs;

            match storage
                .query_range(device_id, metric, start_time, end_time, Some(10_000))
                .await
            {
                Ok(ts_result) => {
                    if ts_result.points.is_empty() {
                        format!(
                            "No data found for {}:{} in last {}",
                            device_id, metric, time_spec
                        )
                    } else {
                        // Use adaptive compression for compact output
                        let compressed = neomind_storage::compress_series_adaptive(
                            &ts_result.points,
                            device_id,
                            metric,
                        );
                        // Cap output size to prevent token overflow in LLM context
                        let output =
                            serde_json::to_string_pretty(&compressed).unwrap_or_else(|_| {
                                format!("{} data points retrieved", ts_result.points.len())
                            });
                        if output.chars().count() > 4000 {
                            let truncated: String = output.chars().take(4000).collect();
                            format!(
                                "{}...(truncated, {} points total)",
                                truncated,
                                ts_result.points.len()
                            )
                        } else {
                            output
                        }
                    }
                }
                Err(e) => format!("Query failed for {}:{}: {}", device_id, metric, e),
            }
        } else {
            "Storage not available for data query".to_string()
        };

        actions.push(neomind_storage::ActionExecuted {
            action_type: "data_query".to_string(),
            description: format!("Query {}.{} data for last {}", device_id, metric, time_spec),
            target: format!("{}:{}", device_id, metric),
            parameters: serde_json::json!({"time_spec": time_spec}),
            success: true,
            result: Some(query_result),
        });
    }

    async fn handle_command_decision(
        &self,
        agent: &AiAgent,
        decision: &Decision,
        actions: &mut Vec<neomind_storage::ActionExecuted>,
    ) {
        if let Some((cmd_type, target_id, command_name)) =
            Self::parse_command_from_action(&decision.action)
        {
            tracing::info!(
                agent_id = %agent.id,
                cmd_type = %cmd_type,
                target_id = %target_id,
                command_name = %command_name,
                decision_action = %decision.action,
                "Executing LLM-specified command"
            );

            match cmd_type.as_str() {
                "extension" => {
                    if let Some(action_executed) = self
                        .execute_extension_command_for_agent(
                            agent,
                            &target_id,
                            &command_name,
                            decision,
                        )
                        .await
                    {
                        actions.push(action_executed);
                    }
                }
                "device" => {
                    let resource_id = format!("{}:{}", target_id, command_name);
                    if let Some(resource) = agent.resources.iter().find(|r| {
                        r.resource_type == ResourceType::Command && r.resource_id == resource_id
                    }) {
                        if let Some(action_executed) =
                            self.execute_single_command(agent, resource, decision).await
                        {
                            actions.push(action_executed);
                        }
                    } else {
                        tracing::warn!(
                            agent_id = %agent.id,
                            resource_id = %resource_id,
                            "No matching command resource found"
                        );
                    }
                }
                _ => {
                    tracing::warn!(
                        agent_id = %agent.id,
                        cmd_type = %cmd_type,
                        "Unknown command type"
                    );
                }
            }
        } else if let Some(cmd_name) = extract_command_from_description(&decision.description) {
            for resource in &agent.resources {
                let is_device_cmd = resource.resource_type == ResourceType::Command
                    && resource.resource_id.ends_with(&format!(":{}", cmd_name));
                let is_ext_cmd = resource.resource_type == ResourceType::ExtensionTool
                    && resource.resource_id.ends_with(&format!(":{}", cmd_name));

                if is_device_cmd || is_ext_cmd {
                    let parts: Vec<&str> = resource.resource_id.split(':').collect();

                    match resource.resource_type {
                        ResourceType::Command => {
                            if parts.len() == 2 {
                                if let Some(action_executed) =
                                    self.execute_single_command(agent, resource, decision).await
                                {
                                    actions.push(action_executed);
                                }
                                break;
                            }
                        }
                        ResourceType::ExtensionTool
                            if parts.len() >= 3 && parts[0] == "extension" =>
                        {
                            if let Some(action_executed) = self
                                .execute_extension_command_for_agent(
                                    agent, parts[1], parts[2], decision,
                                )
                                .await
                            {
                                actions.push(action_executed);
                            }
                            break;
                        }
                        _ => {}
                    }
                }
            }
        } else {
            tracing::warn!(
                agent_id = %agent.id,
                decision_action = %decision.action,
                decision_description = %decision.description,
                "Could not parse command from decision"
            );
        }
    }

    async fn handle_alert_decision(
        &self,
        agent: &AiAgent,
        decision: &Decision,
        notifications: &mut Vec<neomind_storage::NotificationSent>,
    ) {
        tracing::info!(
            agent_id = %agent.id,
            decision_type = %decision.decision_type,
            decision_action = %decision.action,
            "Alert-type decision detected, sending notification"
        );
        self.send_alert_for_decision(agent, decision, notifications)
            .await;
    }

    async fn handle_condition_met_decision(
        &self,
        agent: &AiAgent,
        decision: &Decision,
        actions: &mut Vec<neomind_storage::ActionExecuted>,
        notifications: &mut Vec<neomind_storage::NotificationSent>,
    ) {
        // Execute all device commands
        for resource in &agent.resources {
            if resource.resource_type == ResourceType::Command {
                if let Some(action_executed) =
                    self.execute_single_command(agent, resource, decision).await
                {
                    actions.push(action_executed);
                }
            }
        }

        // Send notifications for alert actions
        let should_send_alert = agent
            .parsed_intent
            .as_ref()
            .map(|i| {
                i.actions.iter().any(|a| {
                    a.contains("alert")
                        || a.contains("notification")
                        || a.contains("报警")
                        || a.contains("通知")
                })
            })
            .unwrap_or(false);

        tracing::debug!(
            agent_id = %agent.id,
            should_send_alert,
            has_parsed_intent = agent.parsed_intent.is_some(),
            actions = ?agent.parsed_intent.as_ref().map(|i| &i.actions),
            has_message_manager = self.message_manager.is_some(),
            "Checking if alert should be sent"
        );

        if should_send_alert {
            self.send_alert_for_decision(agent, decision, notifications)
                .await;
        }
    }

    async fn handle_execute_action_decision(
        &self,
        agent: &AiAgent,
        decision: &Decision,
        actions: &mut Vec<neomind_storage::ActionExecuted>,
    ) {
        let mentioned_command = extract_command_from_description(&decision.description);
        let mentioned_device = extract_device_from_description(&decision.description);

        let commands_to_execute: Vec<_> = agent
            .resources
            .iter()
            .filter(|r| r.resource_type == ResourceType::Command)
            .filter(|r| {
                if let Some(ref cmd_name) = mentioned_command {
                    r.resource_id.ends_with(&format!(":{}", cmd_name))
                        || r.resource_id.contains(cmd_name)
                } else if let Some(ref dev_id) = mentioned_device {
                    r.resource_id.starts_with(&format!("{}:", dev_id))
                } else {
                    true
                }
            })
            .collect();

        if commands_to_execute.is_empty() {
            tracing::warn!(
                agent_id = %agent.id,
                decision_description = %decision.description,
                "No matching commands found for execution"
            );
        } else {
            tracing::info!(
                agent_id = %agent.id,
                command_count = commands_to_execute.len(),
                "Executing {} command(s) from decision",
                commands_to_execute.len()
            );
        }

        for resource in commands_to_execute {
            if let Some(action_executed) =
                self.execute_single_command(agent, resource, decision).await
            {
                actions.push(action_executed);
            }
        }
    }

    pub(crate) async fn execute_decisions(
        &self,
        agent: &AiAgent,
        decisions: &[Decision],
    ) -> AgentResult<(
        Vec<neomind_storage::ActionExecuted>,
        Vec<neomind_storage::NotificationSent>,
    )> {
        let mut actions_executed = Vec::new();
        let mut notifications_sent = Vec::new();

        // Pre-build allowed actions for Focused mode scope validation
        let allowed_command_ids: Vec<String> =
            if agent.execution_mode == neomind_storage::agents::ExecutionMode::Focused {
                agent
                    .resources
                    .iter()
                    .filter(|r| {
                        matches!(
                            r.resource_type,
                            neomind_storage::ResourceType::Command
                                | neomind_storage::ResourceType::ExtensionTool
                        )
                    })
                    .map(|r| r.resource_id.clone())
                    .collect()
            } else {
                Vec::new() // Empty = no scope restriction for Free mode
            };

        for decision in decisions {
            // Scope validation for Focused mode
            if !allowed_command_ids.is_empty() && decision.decision_type == "command" {
                let action = &decision.action;
                let is_allowed = allowed_command_ids.iter().any(|rid| {
                    // 1. Exact match (most reliable)
                    if action == rid {
                        return true;
                    }
                    // 2. Suffix match: action "turn_on" matches rid "light_living:turn_on"
                    if let Some(cmd_suffix) = rid.split(':').next_back() {
                        if action == cmd_suffix {
                            return true;
                        }
                        if action.ends_with(&format!(":{}", cmd_suffix)) {
                            return true;
                        }
                    }
                    // 3. Extension tool format: action contains the full rid
                    if action.contains(rid.as_str()) {
                        return true;
                    }
                    false
                });

                if !is_allowed {
                    tracing::warn!(
                        action = %decision.action,
                        allowed = ?allowed_command_ids,
                        "Focused Mode: rejecting out-of-scope command"
                    );
                    actions_executed.push(neomind_storage::ActionExecuted {
                        action_type: "command".to_string(),
                        description: format!(
                            "Rejected: command '{}' not in bound resources",
                            decision.action
                        ),
                        target: String::new(),
                        parameters: serde_json::json!({}),
                        success: false,
                        result: Some("Rejected: command not in bound resources".to_string()),
                    });
                    continue;
                }
            }

            // Handle query decisions (e.g., "query:device_id:metric:time_range")
            if decision.action.starts_with("query:") {
                self.handle_query_decision(agent, decision, &mut actions_executed)
                    .await;
                continue;
            }

            // Handle LLM-driven command decisions (decision_type == "command")
            if decision.decision_type == "command" {
                self.handle_command_decision(agent, decision, &mut actions_executed)
                    .await;
                continue;
            }

            // Handle alert-type decisions
            if Self::is_alert_decision(decision) {
                self.handle_alert_decision(agent, decision, &mut notifications_sent)
                    .await;
            }

            // Handle condition_met decisions (legacy)
            if decision.decision_type == "condition_met" {
                self.handle_condition_met_decision(
                    agent,
                    decision,
                    &mut actions_executed,
                    &mut notifications_sent,
                )
                .await;
                continue;
            }

            // Handle execute_action decisions
            if Self::is_execute_action(&decision.action) {
                self.handle_execute_action_decision(agent, decision, &mut actions_executed)
                    .await;
            }
        }

        Ok((actions_executed, notifications_sent))
    }

    pub(crate) async fn send_alert_for_decision(
        &self,
        agent: &AiAgent,
        decision: &neomind_storage::Decision,
        notifications_sent: &mut Vec<neomind_storage::NotificationSent>,
    ) {
        let alert_message = format!(
            "Agent '{}' - {}: {}",
            agent.name, decision.decision_type, decision.description
        );

        // Send via MessageManager if available
        if let Some(ref message_manager) = self.message_manager {
            use neomind_messages::{Message, MessageSeverity};

            // Determine if this is an alert or notification based on decision type
            let is_alert = decision.decision_type.to_lowercase().contains("alert")
                || decision.decision_type.to_lowercase().contains("报警")
                || decision.decision_type.to_lowercase().contains("critical")
                || decision.decision_type.to_lowercase().contains("emergency")
                || decision.decision_type.to_lowercase().contains("紧急")
                || decision.decision_type.to_lowercase().contains("warning")
                || decision.decision_type.to_lowercase().contains("警告")
                || decision.decision_type.to_lowercase().contains("error")
                || decision.decision_type.to_lowercase().contains("异常")
                || decision.decision_type.to_lowercase().contains("故障");

            // Determine severity based on decision type
            let severity = if decision.decision_type.to_lowercase().contains("critical")
                || decision.decision_type.to_lowercase().contains("emergency")
                || decision.decision_type.to_lowercase().contains("紧急")
            {
                MessageSeverity::Critical
            } else if decision.decision_type.to_lowercase().contains("warning")
                || decision.decision_type.to_lowercase().contains("警告")
                || decision.decision_type.to_lowercase().contains("error")
            {
                MessageSeverity::Warning
            } else {
                MessageSeverity::Info
            };

            // Create message with appropriate category
            let (category, title_prefix) = if is_alert {
                ("alert", "Agent Alert")
            } else {
                ("notification", "Agent Notification")
            };

            let mut msg = Message::new(
                category,
                severity,
                format!("{}: {}", title_prefix, agent.name),
                alert_message.clone(),
                agent.id.clone(),
            );
            msg.source_type = "agent".to_string();

            tracing::info!(
                agent_id = %agent.id,
                category = %category,
                alert_message = %alert_message,
                severity = ?severity,
                "Sending message via MessageManager"
            );

            match message_manager.create_message(msg).await {
                Ok(msg) => {
                    record_notification(
                        notifications_sent,
                        "message_manager",
                        "configured_channels",
                        alert_message,
                        true,
                    );
                    tracing::info!(
                        agent_id = %agent.id,
                        message_id = %msg.id.to_string(),
                        "Message sent via MessageManager successfully"
                    );
                }
                Err(e) => {
                    record_notification(
                        notifications_sent,
                        "message_manager",
                        "configured_channels",
                        alert_message.clone(),
                        false,
                    );
                    tracing::warn!(
                        agent_id = %agent.id,
                        error = %e,
                        "Failed to send message via MessageManager"
                    );
                }
            }
        } else {
            // Fallback: Publish event to EventBus if MessageManager not available
            tracing::warn!(
                agent_id = %agent.id,
                "MessageManager not available, using EventBus fallback"
            );
            if let Some(ref bus) = self.event_bus {
                let _ = bus
                    .publish(NeoMindEvent::MessageCreated {
                        message_id: uuid::Uuid::new_v4().to_string(),
                        title: format!("Agent Alert: {}", agent.name),
                        severity: "info".to_string(),
                        message: decision.description.clone(),
                        timestamp: chrono::Utc::now().timestamp(),
                    })
                    .await;

                record_notification(
                    notifications_sent,
                    "event_bus",
                    "event_subscribers",
                    alert_message,
                    true,
                );
            }
        }
    }

    pub(crate) async fn maybe_generate_report(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
    ) -> AgentResult<Option<GeneratedReport>> {
        // Only generate reports for report generation agents
        if let Some(ref intent) = agent.parsed_intent {
            if matches!(
                intent.intent_type,
                neomind_storage::IntentType::ReportGeneration
            ) {
                let content = self.generate_report_content(agent, data).await?;

                return Ok(Some(GeneratedReport {
                    report_type: "summary".to_string(),
                    content,
                    data_summary: data
                        .iter()
                        .map(|d| neomind_storage::DataSummary {
                            source: d.source.clone(),
                            metric: d.data_type.clone(),
                            count: 1,
                            statistics: d.values.clone(),
                        })
                        .collect(),
                    generated_at: chrono::Utc::now().timestamp(),
                }));
            }
        }

        Ok(None)
    }

    pub(crate) async fn generate_report_content(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
    ) -> AgentResult<String> {
        let mut report = format!("# {} - Report\n\n", agent.name);
        report.push_str(&format!(
            "Generated: {}\n\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
        ));

        report.push_str("## Data Summary\n\n");
        for data_item in data {
            report.push_str(&format!(
                "- **{}**: {}\n",
                data_item.source, data_item.values
            ));
        }

        report.push_str("\n## Analysis Results\n\n");
        if let Some(ref intent) = agent.parsed_intent {
            report.push_str(&format!("Intent Type: {:?}\n", intent.intent_type));
            report.push_str(&format!("Target Metrics: {:?}\n", intent.target_metrics));
        }

        report.push_str("\n## Conclusion\n\n");
        report.push_str(&agent.user_prompt);

        Ok(report)
    }
}
