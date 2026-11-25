//! Read-only inventory of agents for the V1→V2 purge cutover.
//!
//! Classifies each agent as `AutoMigrate` / `ManualFix` / `Retire` so operators
//! can freeze a对照表 before the offline migration window.

use serde::{Deserialize, Serialize};

use crate::agents::{
    AgentFilter, AgentStore, AiAgent, ExecutionMode, NotifyOn, ResourceType, ScheduleType,
};
use crate::Error;

/// Disposition for the purge对照表.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryDisposition {
    /// Safe to map with [`crate::agents_migrate::map_agent_v1_to_v2`] as-is.
    AutoMigrate,
    /// Needs human review (vision/SameReport, judgment, multi-tool, …).
    ManualFix,
    /// Candidate to drop (empty / long-dead / unrecoverable).
    Retire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryRow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub schedule: String,
    pub execution_mode: String,
    pub disposition: InventoryDisposition,
    pub reasons: Vec<String>,
    pub resource_count: usize,
    pub has_notify: bool,
    pub tool_enabled: Option<bool>,
    pub last_execution_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InventoryReport {
    pub db_path: String,
    pub rows: Vec<InventoryRow>,
    pub auto_migrate: usize,
    pub manual_fix: usize,
    pub retire: usize,
}

impl InventoryReport {
    pub fn to_markdown_table(&self) -> String {
        let mut out = String::new();
        out.push_str("# Agent V1→V2 inventory\n\n");
        out.push_str(&format!("Source: `{}`\n\n", self.db_path));
        out.push_str(&format!(
            "Counts: auto_migrate={} · manual_fix={} · retire={} · total={}\n\n",
            self.auto_migrate,
            self.manual_fix,
            self.retire,
            self.rows.len()
        ));
        out.push_str(
            "| id | name | status | schedule | mode | disposition | reasons | resources |\n",
        );
        out.push_str("|---|---|---|---|---|---|---|---:|\n");
        for r in &self.rows {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | `{:?}` | {} | {} |\n",
                r.id,
                escape_cell(&r.name),
                r.status,
                r.schedule,
                r.execution_mode,
                r.disposition,
                escape_cell(&r.reasons.join("; ")),
                r.resource_count,
            ));
        }
        if self.rows.is_empty() {
            out.push_str("\n_(empty — no agents found; scaffold ready for cutover)_\n");
        }
        out
    }
}

fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Classify a legacy [`AiAgent`] for the purge对照表.
pub fn classify_agent(agent: &AiAgent) -> (InventoryDisposition, Vec<String>) {
    let mut reasons = Vec::new();

    let prompt_l = agent.user_prompt.to_lowercase();
    let visionish = prompt_l.contains("vision")
        || prompt_l.contains("image")
        || prompt_l.contains("frame")
        || prompt_l.contains("camera")
        || prompt_l.contains("图片")
        || prompt_l.contains("摄像头")
        || agent.resources.iter().any(|r| {
            r.resource_id.contains("image")
                || r.resource_id.contains("frame")
                || r.name.to_lowercase().contains("camera")
        });
    if visionish {
        reasons.push("vision/camera/frame binding — verify SameReport vs latest".into());
    }

    let judgment = agent
        .notify
        .as_ref()
        .is_some_and(|n| matches!(n.on, NotifyOn::Judgment));
    if judgment {
        reasons.push("notify.on=judgment — confirm shell/message path after V2".into());
    }

    let multi_tool = agent.tool_config.as_ref().is_some_and(|tc| {
        tc.enabled && (tc.allowed_tools.is_empty() || tc.allowed_tools.len() > 1)
    });
    if multi_tool {
        reasons.push("multi-tool / unrestricted allowlist".into());
    }

    if matches!(agent.schedule.schedule_type, ScheduleType::Event) {
        reasons.push("event trigger — verify filter + resource binding".into());
    }

    if agent
        .resources
        .iter()
        .any(|r| matches!(r.resource_type, ResourceType::Command))
    {
        reasons.push("command resource — map to controlled_actions + idempotent flag".into());
    }

    let emptyish = agent.user_prompt.trim().is_empty()
        || (matches!(agent.status, crate::agents::AgentStatus::Stopped)
            && agent.stats.total_executions == 0
            && agent.last_execution_at.is_none());
    if emptyish {
        reasons.push("empty prompt or never-run Stopped agent".into());
        return (InventoryDisposition::Retire, reasons);
    }

    if matches!(agent.status, crate::agents::AgentStatus::Error)
        && agent.stats.failed_executions > 20
        && agent.stats.successful_executions == 0
    {
        reasons.push("chronic Error with zero successes".into());
        return (InventoryDisposition::Retire, reasons);
    }

    if !reasons.is_empty() {
        return (InventoryDisposition::ManualFix, reasons);
    }

    let simple_mode = matches!(
        agent.execution_mode,
        ExecutionMode::Structured | ExecutionMode::Focused
    );
    let simple_sched = matches!(
        agent.schedule.schedule_type,
        ScheduleType::Interval | ScheduleType::Cron | ScheduleType::Manual
    );
    if simple_mode && simple_sched {
        reasons.push("simple schedule + mode — map_agent_v1_to_v2".into());
        (InventoryDisposition::AutoMigrate, reasons)
    } else {
        reasons.push("free-mode or uncommon shape — spot-check after map".into());
        (InventoryDisposition::ManualFix, reasons)
    }
}

fn row_from_agent(agent: &AiAgent) -> InventoryRow {
    let (disposition, reasons) = classify_agent(agent);
    InventoryRow {
        id: agent.id.clone(),
        name: agent.name.clone(),
        status: format!("{:?}", agent.status),
        schedule: format!("{:?}", agent.schedule.schedule_type),
        execution_mode: format!("{:?}", agent.execution_mode),
        disposition,
        reasons,
        resource_count: agent.resources.len(),
        has_notify: agent.notify.is_some(),
        tool_enabled: agent.tool_config.as_ref().map(|t| t.enabled),
        last_execution_at: agent.last_execution_at,
    }
}

/// Scan `agents.redb` into an inventory report (uses AgentStore dual-read).
pub async fn inventory_agents_db(path: &std::path::Path) -> Result<InventoryReport, Error> {
    let store = AgentStore::open(path)?;
    let agents = store
        .query_agents(AgentFilter {
            status: None,
            schedule_type: None,
            start_time: None,
            end_time: None,
            limit: None,
            offset: None,
        })
        .await?;
    let mut rows: Vec<_> = agents.iter().map(row_from_agent).collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    let auto_migrate = rows
        .iter()
        .filter(|r| r.disposition == InventoryDisposition::AutoMigrate)
        .count();
    let manual_fix = rows
        .iter()
        .filter(|r| r.disposition == InventoryDisposition::ManualFix)
        .count();
    let retire = rows
        .iter()
        .filter(|r| r.disposition == InventoryDisposition::Retire)
        .count();
    Ok(InventoryReport {
        db_path: path.display().to_string(),
        rows,
        auto_migrate,
        manual_fix,
        retire,
    })
}

/// Empty scaffold report (no DB) for operators to fill before cutover.
pub fn empty_inventory_scaffold(db_path_hint: &str) -> InventoryReport {
    InventoryReport {
        db_path: db_path_hint.into(),
        rows: vec![],
        auto_migrate: 0,
        manual_fix: 0,
        retire: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentMemory, AgentSchedule, AgentStats, AgentStatus};

    fn bare(name: &str, prompt: &str) -> AiAgent {
        AiAgent {
            id: "a1".into(),
            name: name.into(),
            description: None,
            user_prompt: prompt.into(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300),
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 100,
            created_at: 1,
            updated_at: 2,
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 5,
            tool_config: None,
            execution_mode: ExecutionMode::Structured,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: None,
            max_tokens_per_run: None,
        }
    }

    #[test]
    fn simple_structured_is_auto() {
        let (d, _) = classify_agent(&bare("meter", "Report average power"));
        assert_eq!(d, InventoryDisposition::AutoMigrate);
    }

    #[test]
    fn vision_prompt_is_manual() {
        let (d, reasons) = classify_agent(&bare("cam", "Check camera frame for people"));
        assert_eq!(d, InventoryDisposition::ManualFix);
        assert!(reasons.iter().any(|r| r.contains("vision")));
    }

    #[test]
    fn empty_prompt_is_retire() {
        let (d, _) = classify_agent(&bare("x", "   "));
        assert_eq!(d, InventoryDisposition::Retire);
    }

    #[test]
    fn markdown_scaffold_has_header() {
        let md = empty_inventory_scaffold("/tmp/agents.redb").to_markdown_table();
        assert!(md.contains("auto_migrate=0"));
        assert!(md.contains("empty"));
    }
}
