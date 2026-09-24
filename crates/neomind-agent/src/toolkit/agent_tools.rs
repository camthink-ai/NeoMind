//! Ask one of the user's AI agents what it last concluded.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use neomind_storage::{AgentStore, AiAgent};

use std::future::Future;
use std::pin::Pin;

use super::error::Result;
use super::tool::{Tool, ToolCategory};
use super::ToolOutput;

/// How a tool asks the platform to run an agent.
///
/// Wired by the API, which owns the agent manager — the toolkit has no business
/// reaching into it, and the manager needs the whole application state to exist
/// before it can be built.
pub type RunAgentCallback = Arc<
    dyn Fn(
            String,
            Option<String>,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<RunOutcome, String>> + Send>>
        + Send
        + Sync,
>;

/// What came of asking an agent to run.
#[derive(Debug, Clone)]
pub enum RunOutcome {
    /// It finished inside the caller's patience window.
    Finished {
        conclusion: String,
        confidence: Option<f32>,
    },
    /// Still going. The run continues in the background and writes its record
    /// as usual; `query_conclusion` reads the result once it lands.
    StillRunning,
}

/// Run one of the user's agents now.
///
/// The other half of [`QueryConclusionTool`]: that one reads the conclusion the
/// agent already reached, this one asks for a fresh look. They are separate
/// tools because the choice between them is the whole point — a model that has
/// only "run the agent" will re-run for a question the agent answered a minute
/// ago, and a model that has only "read the conclusion" has no way to act.
pub struct RunAgentTool {
    store: Arc<AgentStore>,
    run: RunAgentCallback,
}

impl RunAgentTool {
    pub fn new(store: Arc<AgentStore>, run: RunAgentCallback) -> Self {
        Self { store, run }
    }
}

#[async_trait]
impl Tool for RunAgentTool {
    fn name(&self) -> &str {
        "run_now"
    }

    fn description(&self) -> &str {
        "Ask one of the user's AI agents to look at something now. Runs the agent immediately and \
         returns its conclusion if it finishes within a minute; a longer run comes back as \
         started, and you can read its result shortly after with query_conclusion. Use this when \
         the user wants something rechecked or acted on now — after query_conclusion has shown \
         the existing answer is stale, or when they ask about something the agent has not looked \
         at. Do NOT use it to answer a question the agent already answered."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Which agent, as the user referred to it — its name (or part of it) or its id."
                },
                "input": {
                    "type": "string",
                    "description": "Optional extra instruction for this run only (e.g. focus on the loading bay). Most runs need none: the agent already knows what it watches."
                }
            },
            "required": ["agent"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let Some(needle) = args.get("agent").and_then(|v| v.as_str()) else {
            return Ok(ToolOutput::error(
                "Missing required parameter 'agent' — pass the agent's name or id.",
            ));
        };

        let agent = match resolve_agent(&self.store, needle).await {
            Ok(agent) => agent,
            Err(candidates) if candidates.is_empty() => {
                return Ok(ToolOutput::error(format!(
                    "No agent matches '{needle}'. The user's agents are: {}.",
                    agent_names(&self.store).await
                )));
            }
            Err(candidates) => {
                return Ok(ToolOutput::error(format!(
                    "'{needle}' matches more than one agent: {}. Ask the user which one.",
                    candidates.join(", ")
                )));
            }
        };

        let input = args
            .get("input")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        match (self.run)(agent.id.clone(), input).await {
            Ok(RunOutcome::Finished {
                conclusion,
                confidence,
            }) => Ok(ToolOutput::success(json!({
                "agent": agent.name,
                "agent_id": agent.id,
                "ran": true,
                "finished": true,
                "conclusion": conclusion,
                "confidence": confidence,
            }))),
            Ok(RunOutcome::StillRunning) => Ok(ToolOutput::success(json!({
                "agent": agent.name,
                "agent_id": agent.id,
                "ran": true,
                "finished": false,
                "message": "The run is still going and will keep going in the background. \
                            Tell the user it is working and that you will have the result \
                            shortly; read it with query_conclusion once it lands.",
            }))),
            Err(e) => Ok(ToolOutput::error(format!(
                "'{}' could not be run: {e}",
                agent.name
            ))),
        }
    }
}

/// The conclusion an agent already reached, with how long ago.
///
/// This exists because "what does the cold room look like right now" has a
/// cheap answer and an expensive one. The agent has been running on its own
/// schedule and has a conclusion sitting in its last execution; asking for that
/// costs nothing. Invoking the agent to find out costs an inference and a wait.
/// The description below is written to make the cheap path the obvious one —
/// small models pick tools by description, and the difference between a
/// two-second answer and a two-minute one is entirely in what it reads here.
pub struct QueryConclusionTool {
    store: Arc<AgentStore>,
}

impl QueryConclusionTool {
    pub fn new(store: Arc<AgentStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for QueryConclusionTool {
    fn name(&self) -> &str {
        "query_conclusion"
    }

    fn description(&self) -> &str {
        "Ask one of the user's AI agents what it last concluded. Returns the agent's most recent \
         conclusion and how long ago it was reached. Use this FIRST whenever the user asks about \
         the current state of something they have an agent watching (\"is the cold room ok\", \
         \"冷库现在怎么样\") — the agent has already been running on its own schedule, so this is \
         instant and costs nothing. Only invoke an agent (a fresh run) when the user asks for \
         something the agent has not already looked at, or when this answer is clearly out of date \
         and the user wants it rechecked."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Which agent, as the user referred to it — its name (or part of it) or its id."
                }
            },
            "required": ["agent"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let Some(needle) = args.get("agent").and_then(|v| v.as_str()) else {
            return Ok(ToolOutput::error(
                "Missing required parameter 'agent' — pass the agent's name or id.",
            ));
        };

        let agent = match resolve_agent(&self.store, needle).await {
            Ok(agent) => agent,
            Err(candidates) if candidates.is_empty() => {
                return Ok(ToolOutput::error(format!(
                    "No agent matches '{needle}'. The user's agents are: {}. \
                     Use `shell neomind agent list` if you need more detail.",
                    self.names().await
                )));
            }
            Err(candidates) => {
                return Ok(ToolOutput::error(format!(
                    "'{needle}' matches more than one agent: {}. Ask the user which one, \
                     or pass the full name.",
                    candidates.join(", ")
                )));
            }
        };

        let latest = self
            .store
            .get_latest_execution(&agent.id)
            .await
            .map_err(|e| super::error::ToolError::Execution(format!("read executions: {e}")))?;

        let Some(execution) = latest else {
            return Ok(ToolOutput::success(json!({
                "agent": agent.name,
                "has_run": false,
                "message": format!(
                    "'{}' has not run yet, so it has no conclusion to report. \
                     Its schedule is {:?}; it may simply not have reached its first run.",
                    agent.name, agent.schedule.schedule_type
                ),
            })));
        };

        let age_seconds = (chrono::Utc::now().timestamp() - execution.timestamp).max(0);

        // A structured agent stores the model's JSON object. The notification
        // path renders it against the agent's own schema — "运行状态: 异常" — and
        // this path returned it verbatim, so asking chat how the cold room was
        // answered with `{"status":"异常","anomaly_count":2}`. One renderer,
        // both paths; anything the renderer declines falls through unchanged
        // rather than being replaced by an empty message.
        let conclusion = if agent.execution_mode == neomind_storage::ExecutionMode::Structured {
            agent
                .output_schema
                .as_deref()
                .and_then(|schema| {
                    crate::ai_agent::executor::prose_from_structured_conclusion(
                        schema,
                        &execution.decision_process.conclusion,
                    )
                })
                .unwrap_or_else(|| execution.decision_process.conclusion.clone())
        } else {
            execution.decision_process.conclusion.clone()
        };

        Ok(ToolOutput::success(json!({
            "agent": agent.name,
            "agent_id": agent.id,
            "status": format!("{:?}", agent.status),
            "has_run": true,
            "conclusion": conclusion,
            "concluded_at": chrono::DateTime::from_timestamp(execution.timestamp, 0)
                .map(|t| t.to_rfc3339()),
            "age_seconds": age_seconds,
            "age": humanize_age(age_seconds),
            "success": execution.status == neomind_storage::ExecutionStatus::Completed,
            "error": execution.error,
        })))
    }
}

impl QueryConclusionTool {
    /// The user's agents, named, for the "no match" message.
    async fn names(&self) -> String {
        agent_names(&self.store).await
    }
}

/// Find the agent a person referred to: by id, by exact name, or by a
/// distinctive part of the name ("冷库" for "冷库温度盯守").
///
/// Returns the candidates when the reference is ambiguous — guessing between
/// two agents would answer about the wrong one, which is worse than asking.
pub(crate) async fn resolve_agent(
    store: &AgentStore,
    needle: &str,
) -> std::result::Result<AiAgent, Vec<String>> {
    let agents = store
        .query_agents(Default::default())
        .await
        .unwrap_or_default();

    if let Some(agent) = agents.iter().find(|a| a.id == needle) {
        return Ok(agent.clone());
    }

    let needle_lower = needle.trim().to_lowercase();

    if let Some(agent) = agents
        .iter()
        .find(|a| a.name.to_lowercase() == needle_lower)
    {
        return Ok(agent.clone());
    }

    let partial: Vec<&AiAgent> = agents
        .iter()
        .filter(|a| a.name.to_lowercase().contains(&needle_lower))
        .collect();

    match partial.as_slice() {
        [only] => Ok((*only).clone()),
        [] => Err(vec![]),
        many => Err(many.iter().map(|a| a.name.clone()).collect()),
    }
}

/// The user's agents, named, for a "no such agent" message that leaves the
/// model somewhere to go.
pub(crate) async fn agent_names(store: &AgentStore) -> String {
    let agents = store
        .query_agents(Default::default())
        .await
        .unwrap_or_default();
    if agents.is_empty() {
        return "none — no agents have been created yet".to_string();
    }
    agents
        .iter()
        .map(|a| a.name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

/// `45s ago`, `12m ago`, `3h20m ago`, `2d ago`.
fn humanize_age(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{s} seconds ago"),
        s if s < 3600 => format!("{} minutes ago", s / 60),
        s if s < 86_400 => {
            let (h, m) = (s / 3600, (s % 3600) / 60);
            if m == 0 {
                format!("{h} hours ago")
            } else {
                format!("{h}h{m}m ago")
            }
        }
        s => format!("{} days ago", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn agent(id: &str, name: &str) -> AiAgent {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "user_prompt": "watch it",
            "resources": [],
            "schedule": { "schedule_type": "interval", "interval_seconds": 60 },
            "status": "active",
            "created_at": 0,
            "updated_at": 0,
            "stats": {
                "total_executions": 0, "successful_executions": 0,
                "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": null
            },
            "memory": {},
        }))
        .expect("agent fixture")
    }

    async fn tool_with(names: &[(&str, &str)]) -> QueryConclusionTool {
        let store = AgentStore::memory().expect("memory store");
        for (id, name) in names {
            store.save_agent(&agent(id, name)).await.expect("save");
        }
        QueryConclusionTool::new(store)
    }

    /// What a structured agent's conclusion looks like when chat reads it back.
    ///
    /// A structured run stores the model's JSON object. The notification path
    /// renders it against the agent's own schema — "运行状态: 异常" — and this
    /// path returned it verbatim, so asking chat how the cold room was answered
    /// with `{"status":"异常","anomaly_count":2}` while the notification for the
    /// very same run read as a sentence. Same renderer now, both paths.
    #[tokio::test]
    async fn a_structured_conclusion_reads_back_as_prose() {
        let store = seed_structured_with_conclusion(r#"{"status":"异常","anomaly_count":2}"#).await;
        let out = QueryConclusionTool::new(store)
            .execute(serde_json::json!({"agent": "冷库"}))
            .await
            .expect("tool runs");

        let conclusion = out.data["conclusion"].as_str().expect("a conclusion");
        assert_eq!(
            conclusion, "运行状态: 异常\n异常数量: 2项",
            "the field descriptions and unit are what makes this readable"
        );
        assert!(
            !conclusion.starts_with('{'),
            "raw JSON is what this test exists to keep out of the answer"
        );
    }

    /// The renderer declines anything it does not understand, and the stored
    /// text is what the user gets — never an empty string.
    #[tokio::test]
    async fn an_unrenderable_conclusion_falls_through_unchanged() {
        let store = seed_structured_with_conclusion("模型这次只回了一句话").await;
        let out = QueryConclusionTool::new(store)
            .execute(serde_json::json!({"agent": "冷库"}))
            .await
            .expect("tool runs");
        assert_eq!(out.data["conclusion"], "模型这次只回了一句话");
    }

    /// Agents that are not structured store prose already; nothing to render.
    #[tokio::test]
    async fn a_free_agents_conclusion_is_left_alone() {
        let store = AgentStore::memory().expect("store");
        let a = agent("f1", "巡检");
        store.save_agent(&a).await.expect("save");
        store
            .save_execution(&execution_record("f1", "冷库一切正常"))
            .await
            .expect("save execution");

        let out = QueryConclusionTool::new(store)
            .execute(serde_json::json!({"agent": "巡检"}))
            .await
            .expect("tool runs");
        assert_eq!(out.data["conclusion"], "冷库一切正常");
    }

    fn execution_record(agent_id: &str, conclusion: &str) -> neomind_storage::AgentExecutionRecord {
        neomind_storage::AgentExecutionRecord {
            id: "e1".into(),
            agent_id: agent_id.into(),
            timestamp: chrono::Utc::now().timestamp(),
            trigger_type: "schedule".into(),
            status: neomind_storage::ExecutionStatus::Completed,
            decision_process: neomind_storage::DecisionProcess {
                situation_analysis: String::new(),
                data_collected: vec![],
                reasoning_steps: vec![],
                decisions: vec![],
                conclusion: conclusion.into(),
                confidence: None,
                stop_reason: String::new(),
            },
            result: None,
            duration_ms: 1,
            error: None,
        }
    }

    /// A structured agent with a two-field schema, and one run recorded against
    /// it carrying `conclusion`.
    async fn seed_structured_with_conclusion(conclusion: &str) -> Arc<AgentStore> {
        use neomind_storage::{ExecutionMode, OperatorField, OperatorFieldType};

        let store = AgentStore::memory().expect("store");
        let mut a = agent("s1", "冷库监控");
        a.execution_mode = ExecutionMode::Structured;
        a.output_schema = Some(vec![
            OperatorField {
                name: "status".into(),
                field_type: OperatorFieldType::Enum(vec!["正常".into(), "异常".into()]),
                unit: None,
                description: Some("运行状态".into()),
            },
            OperatorField {
                name: "anomaly_count".into(),
                field_type: OperatorFieldType::Number,
                unit: Some("项".into()),
                description: Some("异常数量".into()),
            },
        ]);
        store.save_agent(&a).await.expect("save agent");
        store
            .save_execution(&execution_record("s1", conclusion))
            .await
            .expect("save execution");
        store
    }

    /// The user says "冷库", the agent is called "冷库温度盯守" — resolving that
    /// is the whole point of taking a name rather than an id.
    #[tokio::test]
    async fn a_partial_name_finds_the_agent() {
        let tool = tool_with(&[("a1", "冷库温度盯守"), ("a2", "车间能耗预警")]).await;

        assert!(resolve_agent(&tool.store, "冷库").await.is_ok());
        assert!(
            resolve_agent(&tool.store, "a2").await.is_ok(),
            "an id works too"
        );
        assert!(
            resolve_agent(&tool.store, "车间能耗预警").await.is_ok(),
            "so does the full name"
        );
    }

    /// Answering about the wrong agent is worse than asking which one.
    #[tokio::test]
    async fn an_ambiguous_name_is_refused_with_the_candidates() {
        let tool = tool_with(&[("a1", "冷库温度盯守"), ("a2", "冷库湿度盯守")]).await;

        let candidates = resolve_agent(&tool.store, "冷库")
            .await
            .expect_err("ambiguous");
        assert_eq!(candidates.len(), 2, "both are offered: {candidates:?}");
    }

    /// An agent that has not run has no conclusion — and saying so is more
    /// useful than an empty answer the model might paper over.
    #[tokio::test]
    async fn an_agent_that_never_ran_says_so() {
        let tool = tool_with(&[("a1", "冷库温度盯守")]).await;

        let out = tool
            .execute(serde_json::json!({ "agent": "冷库" }))
            .await
            .expect("no failure");

        assert!(out.success);
        assert_eq!(out.data["has_run"], false);
    }

    /// An unknown name must list what does exist, or the model has nowhere to go.
    #[tokio::test]
    async fn an_unknown_name_lists_the_agents_that_do_exist() {
        let tool = tool_with(&[("a1", "冷库温度盯守"), ("a2", "车间能耗预警")]).await;

        let out = tool
            .execute(serde_json::json!({ "agent": "门禁" }))
            .await
            .expect("no failure");

        assert!(!out.success);
        let error = out.error.expect("an error");
        assert!(error.contains("冷库温度盯守"), "{error}");
        assert!(error.contains("车间能耗预警"), "{error}");
    }
}

#[cfg(test)]
mod run_now_tests {
    use super::*;
    use std::sync::Mutex;

    fn agent(id: &str, name: &str) -> AiAgent {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "user_prompt": "watch it",
            "resources": [],
            "schedule": { "schedule_type": "interval", "interval_seconds": 60 },
            "status": "active",
            "created_at": 0, "updated_at": 0,
            "stats": {
                "total_executions": 0, "successful_executions": 0,
                "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": null
            },
            "memory": {},
        }))
        .expect("agent fixture")
    }

    /// Records what it was asked to run, so the test can assert the tool
    /// resolved the user's words to the right agent.
    struct Recorder {
        seen: Mutex<Vec<String>>,
        outcome: RunOutcome,
    }

    async fn tool_with(
        names: &[(&str, &str)],
        outcome: RunOutcome,
    ) -> (RunAgentTool, Arc<Recorder>) {
        let store = AgentStore::memory().expect("memory store");
        for (id, name) in names {
            store.save_agent(&agent(id, name)).await.expect("save");
        }

        let recorder = Arc::new(Recorder {
            seen: Mutex::new(Vec::new()),
            outcome,
        });
        let rec = recorder.clone();
        let run: RunAgentCallback = Arc::new(move |agent_id: String, _input: Option<String>| {
            let rec = rec.clone();
            Box::pin(async move {
                rec.seen.lock().unwrap().push(agent_id);
                Ok(rec.outcome.clone())
            })
        });

        (RunAgentTool::new(store, run), recorder)
    }

    /// The user says "冷库", the agent is called "冷库温度盯守" — the tool has to
    /// make that jump before it runs anything.
    #[tokio::test]
    async fn running_by_a_partial_name_reaches_the_right_agent() {
        let (tool, recorder) = tool_with(
            &[("a1", "冷库温度盯守"), ("a2", "车间能耗预警")],
            RunOutcome::Finished {
                conclusion: "正常".into(),
                confidence: Some(0.9),
            },
        )
        .await;

        let out = tool
            .execute(serde_json::json!({ "agent": "冷库" }))
            .await
            .expect("no failure");

        assert!(out.success);
        assert_eq!(out.data["agent"], "冷库温度盯守");
        assert_eq!(out.data["conclusion"], "正常");
        assert_eq!(recorder.seen.lock().unwrap().as_slice(), ["a1"]);
    }

    /// A run that outlives the wait must not be reported as an answer — the
    /// model would read it out as one.
    #[tokio::test]
    async fn a_run_that_is_still_going_says_so_rather_than_inventing_an_answer() {
        let (tool, _recorder) =
            tool_with(&[("a1", "冷库温度盯守")], RunOutcome::StillRunning).await;

        let out = tool
            .execute(serde_json::json!({ "agent": "冷库" }))
            .await
            .expect("no failure");

        assert!(out.success);
        assert_eq!(out.data["finished"], false);
        assert!(out.data.get("conclusion").is_none(), "{:?}", out.data);
    }

    /// Running the wrong agent is worse than asking which one.
    #[tokio::test]
    async fn an_ambiguous_name_runs_nothing() {
        let (tool, recorder) = tool_with(
            &[("a1", "冷库温度盯守"), ("a2", "冷库湿度盯守")],
            RunOutcome::StillRunning,
        )
        .await;

        let out = tool
            .execute(serde_json::json!({ "agent": "冷库" }))
            .await
            .expect("no failure");

        assert!(!out.success);
        assert!(
            recorder.seen.lock().unwrap().is_empty(),
            "nothing may run when the reference is ambiguous"
        );
    }
}
