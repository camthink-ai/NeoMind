//! Structured chat reply attachments (v1).
//!
//! Server-side extraction of verified reply attachments from successful tool
//! executions in a turn:
//! - `media` (`image`): fallback `/api/images/...` outputs not already inlined in
//!   the assistant's markdown content;
//! - `card` (`device` | `rule` | `transform` | `agent`): high-signal write /
//!   execution outcomes (`created`, `updated`, `executed`);
//! - `chip` (`device` | `rule` | `transform` | `agent`): lightweight clickable
//!   references for read-only lookups (`queried`, `deleted`).
//!
//! Guarantees:
//! - LLM prose is never parsed for structured entity facts;
//! - entity IDs are hydrated against `ServerState` (non-existent IDs are dropped
//!   unless `deleted`);
//! - `list` commands never explode into per-item cards;
//! - same entity touched multiple times in a turn merges into a single entry,
//!   upgrading `queried` -> `created`/`updated`/`executed`;
//! - image URLs already rendered in markdown `![...](url)` are deduped.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::handlers::ServerState;

pub const ATTACHMENT_VERSION_V1: u32 = 1;
const MAX_MEDIA_ATTACHMENTS: usize = 8;
const MAX_CARD_ATTACHMENTS: usize = 12;
const MAX_CHIP_ATTACHMENTS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatAttachmentKind {
    Image,
    Device,
    Rule,
    Transform,
    Agent,
    Dashboard,
}

impl ChatAttachmentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Device => "device",
            Self::Rule => "rule",
            Self::Transform => "transform",
            Self::Agent => "agent",
            Self::Dashboard => "dashboard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatAttachmentAction {
    Queried,
    Created,
    Updated,
    Executed,
    Deleted,
}

impl ChatAttachmentAction {
    fn priority(&self) -> u8 {
        match self {
            Self::Queried => 1,
            Self::Executed => 3,
            Self::Updated => 3,
            Self::Created => 4,
            Self::Deleted => 5,
        }
    }

    fn default_display(&self, kind: ChatAttachmentKind) -> ChatAttachmentDisplay {
        if kind == ChatAttachmentKind::Image {
            return ChatAttachmentDisplay::Media;
        }
        match self {
            Self::Created | Self::Updated | Self::Executed => ChatAttachmentDisplay::Card,
            Self::Queried | Self::Deleted => ChatAttachmentDisplay::Chip,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatAttachmentDisplay {
    Media,
    Card,
    Chip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAttachmentStatus {
    pub label: String,
    /// Visual tone: "ok" | "warn" | "error" | "muted" | "running"
    pub tone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachment {
    pub id: String,
    pub version: u32,
    pub kind: ChatAttachmentKind,
    pub action: ChatAttachmentAction,
    pub display: ChatAttachmentDisplay,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ChatAttachmentStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep_link: Option<String>,
    pub snapshot: Value,
}

#[derive(Debug, Clone)]
struct PendingToolCall {
    tool: String,
    arguments: Value,
}

#[derive(Debug, Clone)]
struct EntityCandidate {
    kind: ChatAttachmentKind,
    entity_id: String,
    action: ChatAttachmentAction,
    fallback_title: Option<String>,
    extra_subtitle: Option<String>,
    order: usize,
}

#[derive(Debug, Clone)]
struct ImageCandidate {
    url: String,
    source_label: Option<String>,
    order: usize,
}

/// Accumulates tool starts/ends during a single assistant turn and extracts
/// candidate entities & images without touching LLM prose.
#[derive(Debug, Default)]
pub struct TurnAttachmentCollector {
    in_flight: Vec<PendingToolCall>,
    entities: HashMap<String, EntityCandidate>,
    images: HashMap<String, ImageCandidate>,
    seq: usize,
}

impl TurnAttachmentCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_tool_start(&mut self, tool: &str, arguments: &Value) {
        self.in_flight.push(PendingToolCall {
            tool: tool.to_string(),
            arguments: arguments.clone(),
        });
    }

    pub fn record_tool_end(&mut self, tool: &str, result: &str, success: bool) {
        let args = self
            .in_flight
            .iter()
            .position(|p| p.tool == tool)
            .map(|idx| self.in_flight.remove(idx).arguments)
            .unwrap_or(Value::Null);

        if !success || !tool_result_succeeded(tool, result) {
            return;
        }

        // 1. Extract durable `/api/images/...` outputs from tool results,
        //    skipping bulk exploratory `list`/`types` CLI commands so finding an
        //    entity ID never dumps unrelated camera thumbnails under the reply.
        let is_bulk_discovery = tool == "shell"
            && args
                .get("command")
                .and_then(|v| v.as_str())
                .is_some_and(is_bulk_discovery_shell_command);
        if !is_bulk_discovery {
            let source_label = derive_image_source_label(tool, &args);
            for url in extract_image_urls(result) {
                self.upsert_image(url, source_label.clone());
            }
        }

        // 2. Extract entity candidate from `shell` (neomind CLI) or direct domain tool.
        if tool == "shell" {
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                if let Some((kind, action, entity_id, fallback_title, extra_subtitle)) =
                    extract_from_shell_command(cmd, result)
                {
                    self.upsert_entity(kind, entity_id, action, fallback_title, extra_subtitle);
                }
            }
        } else if let Some((kind, action, entity_id, fallback_title)) =
            extract_from_domain_tool(tool, &args, result)
        {
            self.upsert_entity(kind, entity_id, action, fallback_title, None);
        }
    }

    fn upsert_entity(
        &mut self,
        kind: ChatAttachmentKind,
        entity_id: String,
        action: ChatAttachmentAction,
        fallback_title: Option<String>,
        extra_subtitle: Option<String>,
    ) {
        let trimmed = entity_id.trim();
        if trimmed.is_empty()
            || trimmed.len() > 160
            || trimmed.contains('/')
            || trimmed.contains('\\')
            || trimmed.contains("..")
            || trimmed.contains('?')
            || trimmed.contains('#')
            || trimmed.chars().any(char::is_whitespace)
        {
            return;
        }
        let key = format!("{}:{}", kind.as_str(), trimmed);
        self.seq += 1;
        let order = self.seq;
        match self.entities.get_mut(&key) {
            Some(existing) => {
                if action.priority() >= existing.action.priority() {
                    existing.action = action;
                    if extra_subtitle.is_some() {
                        existing.extra_subtitle = extra_subtitle;
                    }
                }
                if existing.fallback_title.is_none() {
                    existing.fallback_title = fallback_title;
                }
            }
            None => {
                self.entities.insert(
                    key,
                    EntityCandidate {
                        kind,
                        entity_id: trimmed.to_string(),
                        action,
                        fallback_title,
                        extra_subtitle,
                        order,
                    },
                );
            }
        }
    }

    fn upsert_image(&mut self, url: String, source_label: Option<String>) {
        if self.images.contains_key(&url) {
            return;
        }
        self.seq += 1;
        self.images.insert(
            url.clone(),
            ImageCandidate {
                url,
                source_label,
                order: self.seq,
            },
        );
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.images.is_empty()
    }
}

/// Finalize collected candidates into verified, hydrated, deduped attachments.
pub async fn finalize_turn_attachments(
    state: &ServerState,
    collector: TurnAttachmentCollector,
    final_content: &str,
) -> Vec<ChatAttachment> {
    if collector.is_empty() {
        return Vec::new();
    }

    // 1. Media attachments — dedupe against images already rendered in markdown.
    let inlined_images = extract_markdown_image_urls(final_content);
    let mut image_candidates: Vec<ImageCandidate> = collector
        .images
        .into_values()
        .filter(|img| !inlined_images.contains(&img.url))
        .collect();
    image_candidates.sort_by_key(|c| c.order);

    let mut media: Vec<ChatAttachment> = image_candidates
        .into_iter()
        .take(MAX_MEDIA_ATTACHMENTS)
        .map(|img| {
            let filename = img.url.rsplit('/').next().unwrap_or("image").to_string();
            ChatAttachment {
                id: format!("image:{}", img.url),
                version: ATTACHMENT_VERSION_V1,
                kind: ChatAttachmentKind::Image,
                action: ChatAttachmentAction::Created,
                display: ChatAttachmentDisplay::Media,
                entity_id: None,
                title: img.source_label.clone().unwrap_or(filename),
                subtitle: Some(img.url.clone()),
                status: None,
                image_url: Some(img.url.clone()),
                deep_link: None,
                snapshot: json!({
                    "url": img.url,
                    "source": img.source_label,
                }),
            }
        })
        .collect();

    // 2. Entity candidates — hydrate authoritatively against ServerState.
    let mut entity_candidates: Vec<EntityCandidate> = collector.entities.into_values().collect();
    entity_candidates.sort_by_key(|c| c.order);

    let mut cards: Vec<ChatAttachment> = Vec::new();
    let mut chips: Vec<ChatAttachment> = Vec::new();

    for cand in entity_candidates {
        if let Some(att) = hydrate_entity(state, cand).await {
            match att.display {
                ChatAttachmentDisplay::Card => {
                    if cards.len() < MAX_CARD_ATTACHMENTS {
                        cards.push(att);
                    }
                }
                ChatAttachmentDisplay::Chip => {
                    if chips.len() < MAX_CHIP_ATTACHMENTS {
                        chips.push(att);
                    }
                }
                ChatAttachmentDisplay::Media => {
                    if media.len() < MAX_MEDIA_ATTACHMENTS {
                        media.push(att);
                    }
                }
            }
        }
    }

    let mut out = Vec::with_capacity(media.len() + cards.len() + chips.len());
    out.append(&mut media);
    out.append(&mut cards);
    out.append(&mut chips);
    out
}

async fn hydrate_entity(state: &ServerState, cand: EntityCandidate) -> Option<ChatAttachment> {
    let display = cand.action.default_display(cand.kind);
    let id = format!("{}:{}", cand.kind.as_str(), cand.entity_id);

    match cand.kind {
        ChatAttachmentKind::Device => {
            match state
                .devices
                .service
                .get_device_with_template(&cand.entity_id)
                .await
            {
                Ok((config, _template)) => {
                    let status = state.devices.service.get_device_status(&cand.entity_id).await;
                    let is_online = matches!(
                        status.status,
                        neomind_devices::ConnectionStatus::Connected
                    );
                    let title = if config.name.trim().is_empty() {
                        cand.entity_id.clone()
                    } else {
                        config.name.clone()
                    };
                    let subtitle = cand.extra_subtitle.or_else(|| {
                        let mut parts = Vec::new();
                        if !config.device_type.is_empty() {
                            parts.push(config.device_type.clone());
                        }
                        if !config.adapter_type.is_empty() {
                            parts.push(config.adapter_type.clone());
                        }
                        if parts.is_empty() {
                            None
                        } else {
                            Some(parts.join(" · "))
                        }
                    });
                    Some(ChatAttachment {
                        id,
                        version: ATTACHMENT_VERSION_V1,
                        kind: ChatAttachmentKind::Device,
                        action: cand.action,
                        display,
                        entity_id: Some(cand.entity_id.clone()),
                        title,
                        subtitle,
                        status: Some(ChatAttachmentStatus {
                            label: if is_online {
                                "online".into()
                            } else {
                                "offline".into()
                            },
                            tone: if is_online {
                                "ok".into()
                            } else {
                                "muted".into()
                            },
                        }),
                        image_url: None,
                        deep_link: Some(format!("/devices/{}", cand.entity_id)),
                        snapshot: json!({
                            "deviceId": config.device_id,
                            "name": config.name,
                            "deviceType": config.device_type,
                            "adapterType": config.adapter_type,
                            "online": is_online,
                            "lastSeen": config.last_seen,
                        }),
                    })
                }
                Err(_) if cand.action == ChatAttachmentAction::Deleted => Some(deleted_attachment(
                    id,
                    cand,
                    Some("/devices".to_string()),
                )),
                Err(_) => None,
            }
        }
        ChatAttachmentKind::Rule => {
            let parsed_id = neomind_rules::RuleId::from_string(&cand.entity_id).ok();
            let rule = match parsed_id {
                Some(ref rid) => state.automation.rule_engine.get_rule(rid).await,
                None => None,
            };
            match rule {
                Some(rule) => {
                    let title = if rule.name.trim().is_empty() {
                        cand.entity_id.clone()
                    } else {
                        rule.name.clone()
                    };
                    let subtitle = cand.extra_subtitle.or_else(|| {
                        if rule.state.trigger_count > 0 {
                            Some(format!("triggers: {}", rule.state.trigger_count))
                        } else {
                            None
                        }
                    });
                    Some(ChatAttachment {
                        id,
                        version: ATTACHMENT_VERSION_V1,
                        kind: ChatAttachmentKind::Rule,
                        action: cand.action,
                        display,
                        entity_id: Some(cand.entity_id.clone()),
                        title,
                        subtitle,
                        status: Some(ChatAttachmentStatus {
                            label: if rule.enabled {
                                "enabled".into()
                            } else {
                                "disabled".into()
                            },
                            tone: if rule.enabled {
                                "ok".into()
                            } else {
                                "muted".into()
                            },
                        }),
                        image_url: None,
                        deep_link: Some("/automation".to_string()),
                        snapshot: json!({
                            "id": cand.entity_id,
                            "name": rule.name,
                            "enabled": rule.enabled,
                            "triggerCount": rule.state.trigger_count,
                            "lastTriggered": rule.state.last_triggered.map(|t| t.to_rfc3339()),
                        }),
                    })
                }
                None if cand.action == ChatAttachmentAction::Deleted => Some(deleted_attachment(
                    id,
                    cand,
                    Some("/automation".to_string()),
                )),
                None => None,
            }
        }
        ChatAttachmentKind::Transform => {
            let store = state.automation.automation_store.as_ref();
            let found = match store {
                Some(s) => s.get_automation(&cand.entity_id).await.ok().flatten(),
                None => None,
            };
            match found {
                Some(t) => {
                    let title = if t.metadata.name.trim().is_empty() {
                        cand.entity_id.clone()
                    } else {
                        t.metadata.name.clone()
                    };
                    let scope = t.scope.as_str().to_string();
                    let subtitle = cand.extra_subtitle.or_else(|| {
                        if scope.is_empty() {
                            None
                        } else {
                            Some(scope.clone())
                        }
                    });
                    Some(ChatAttachment {
                        id,
                        version: ATTACHMENT_VERSION_V1,
                        kind: ChatAttachmentKind::Transform,
                        action: cand.action,
                        display,
                        entity_id: Some(cand.entity_id.clone()),
                        title,
                        subtitle,
                        status: Some(ChatAttachmentStatus {
                            label: if t.metadata.enabled {
                                "enabled".into()
                            } else {
                                "disabled".into()
                            },
                            tone: if t.metadata.enabled {
                                "ok".into()
                            } else {
                                "muted".into()
                            },
                        }),
                        image_url: None,
                        deep_link: Some("/automation/transforms".to_string()),
                        snapshot: json!({
                            "id": t.metadata.id,
                            "name": t.metadata.name,
                            "enabled": t.metadata.enabled,
                            "scope": scope,
                            "description": truncate_chars(&t.metadata.description, 200),
                        }),
                    })
                }
                None if cand.action == ChatAttachmentAction::Deleted => Some(deleted_attachment(
                    id,
                    cand,
                    Some("/automation/transforms".to_string()),
                )),
                None => None,
            }
        }
        ChatAttachmentKind::Agent => {
            let rt = state.require_clean_slate().await.ok();
            let found = if let Some(ref r) = rt {
                let _ = r.ensure_loaded(&cand.entity_id);
                r.get(&cand.entity_id).ok()
            } else {
                None
            };
            match (rt, found) {
                (Some(rt), Some((spec, _plan))) => {
                    let raw_status = rt
                        .runtime_status(&spec.id)
                        .map(|s| format!("{s:?}").to_lowercase())
                        .unwrap_or_else(|| "idle".into());
                    let tone = match raw_status.as_str() {
                        "executing" | "running" => "running",
                        "active" | "idle" | "ready" => "ok",
                        "error" | "failed" => "error",
                        "paused" | "disabled" => "muted",
                        _ => "muted",
                    };
                    let title = if spec.name.trim().is_empty() {
                        cand.entity_id.clone()
                    } else {
                        spec.name.clone()
                    };
                    let desc = spec.description.as_deref().unwrap_or("").trim();
                    let subtitle = cand.extra_subtitle.or_else(|| {
                        if desc.is_empty() {
                            None
                        } else {
                            Some(truncate_chars(desc, 72))
                        }
                    });
                    Some(ChatAttachment {
                        id,
                        version: ATTACHMENT_VERSION_V1,
                        kind: ChatAttachmentKind::Agent,
                        action: cand.action,
                        display,
                        entity_id: Some(cand.entity_id.clone()),
                        title,
                        subtitle,
                        status: Some(ChatAttachmentStatus {
                            label: raw_status.clone(),
                            tone: tone.to_string(),
                        }),
                        image_url: None,
                        deep_link: Some("/agents".to_string()),
                        snapshot: json!({
                            "id": spec.id,
                            "name": spec.name,
                            "status": raw_status,
                            "description": truncate_chars(desc, 200),
                        }),
                    })
                }
                _ if cand.action == ChatAttachmentAction::Deleted => Some(deleted_attachment(
                    id,
                    cand,
                    Some("/agents".to_string()),
                )),
                _ => None,
            }
        }
        ChatAttachmentKind::Dashboard => {
            let loaded = match state.dashboard_store.load(&cand.entity_id) {
                Ok(Some(d)) => Some(d),
                Ok(None) => state
                    .dashboard_store
                    .list_all()
                    .ok()
                    .and_then(|all| all.into_iter().find(|d| d.name == cand.entity_id)),
                Err(_) => None,
            };
            match loaded {
                Some(d) => {
                    let canonical_id = d.id.clone();
                    let title = if d.name.trim().is_empty() {
                        canonical_id.clone()
                    } else {
                        d.name.clone()
                    };
                    let comp_count = d.components.len();
                    let subtitle = cand.extra_subtitle.or_else(|| {
                        Some(format!(
                            "{} component{}",
                            comp_count,
                            if comp_count == 1 { "" } else { "s" }
                        ))
                    });
                    Some(ChatAttachment {
                        id: format!("dashboard:{}", canonical_id),
                        version: ATTACHMENT_VERSION_V1,
                        kind: ChatAttachmentKind::Dashboard,
                        action: cand.action,
                        display,
                        entity_id: Some(canonical_id.clone()),
                        title,
                        subtitle,
                        status: None,
                        image_url: None,
                        deep_link: Some(format!("/visual-dashboard/{}", canonical_id)),
                        snapshot: json!({
                            "id": canonical_id,
                            "name": d.name,
                            "description": d.description,
                            "componentCount": comp_count,
                            "isDefault": d.is_default.unwrap_or(false),
                            "updatedAt": d.updated_at,
                        }),
                    })
                }
                None if cand.action == ChatAttachmentAction::Deleted => Some(deleted_attachment(
                    id,
                    cand,
                    Some("/visual-dashboard".to_string()),
                )),
                None => None,
            }
        }
        ChatAttachmentKind::Image => None,
    }
}

fn deleted_attachment(
    id: String,
    cand: EntityCandidate,
    deep_link: Option<String>,
) -> ChatAttachment {
    let title = cand
        .fallback_title
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| cand.entity_id.clone());
    ChatAttachment {
        id,
        version: ATTACHMENT_VERSION_V1,
        kind: cand.kind,
        action: ChatAttachmentAction::Deleted,
        display: ChatAttachmentDisplay::Chip,
        entity_id: Some(cand.entity_id.clone()),
        title,
        subtitle: None,
        status: Some(ChatAttachmentStatus {
            label: "deleted".into(),
            tone: "muted".into(),
        }),
        image_url: None,
        deep_link,
        snapshot: json!({
            "id": cand.entity_id,
            "deleted": true,
        }),
    }
}

// ============================================================================
// Parsing Helpers (pure, unit-testable)
// ============================================================================

fn tool_result_succeeded(tool: &str, result: &str) -> bool {
    let Ok(val) = serde_json::from_str::<Value>(result) else {
        return true;
    };
    let Some(obj) = val.as_object() else {
        return true;
    };
    if tool == "shell" || obj.contains_key("exit_code") || obj.contains_key("timed_out") {
        if obj.get("timed_out").and_then(|v| v.as_bool()) == Some(true) {
            return false;
        }
        if let Some(code) = obj.get("exit_code").and_then(|v| v.as_i64()) {
            if code != 0 {
                return false;
            }
        }
        // Check inner CLI JSON envelope in stdout if present.
        if let Some(stdout) = obj.get("stdout").and_then(|v| v.as_str()) {
            if let Ok(inner) = serde_json::from_str::<Value>(stdout.trim()) {
                if inner.get("success").and_then(|v| v.as_bool()) == Some(false) {
                    return false;
                }
                if json_reports_sent_false(&inner) {
                    return false;
                }
            }
        }
    }
    if obj.get("success").and_then(|v| v.as_bool()) == Some(false) {
        return false;
    }
    if json_reports_sent_false(&val) {
        return false;
    }
    if let Some(err) = obj.get("error") {
        if err.is_string() && !err.as_str().unwrap_or("").is_empty() {
            return false;
        }
    }
    true
}

fn json_reports_sent_false(val: &Value) -> bool {
    if val.get("sent").and_then(|v| v.as_bool()) == Some(false) {
        return true;
    }
    val.get("data")
        .and_then(|d| d.get("sent"))
        .and_then(|v| v.as_bool())
        == Some(false)
}

/// Skip leading global CLI flags after `neomind` (e.g. `neomind --json device get ...`)
/// and return the index of `<domain>`.
fn neomind_domain_index(tokens: &[String]) -> Option<usize> {
    if tokens.first().map(String::as_str) != Some("neomind") {
        return None;
    }
    let mut i = 1;
    while i < tokens.len() {
        let tok = tokens[i].as_str();
        if matches!(tok, "--json" | "-j" | "--human" | "--quiet" | "-q") {
            i += 1;
        } else if matches!(tok, "--url" | "--token" | "--api-key" | "--format" | "-f") {
            i += 2;
        } else if tok.starts_with("--") && tok.contains('=') {
            i += 1;
        } else {
            return Some(i);
        }
    }
    None
}

fn is_bulk_discovery_shell_command(cmd: &str) -> bool {
    let Some(tokens) = tokenize_simple_command(cmd.trim()) else {
        return false;
    };
    let Some(dom_idx) = neomind_domain_index(&tokens) else {
        return false;
    };
    let Some(verb) = tokens.get(dom_idx + 1).map(String::as_str) else {
        return false;
    };
    matches!(
        verb,
        "list" | "types" | "virtual-metrics" | "data-sources" | "plan-preview"
    )
}

fn derive_image_source_label(tool: &str, args: &Value) -> Option<String> {
    if tool == "image_edit" {
        return Some("Edited image".to_string());
    }
    if tool == "shell" {
        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            let tokens = tokenize_simple_command(cmd)?;
            let dom_idx = neomind_domain_index(&tokens)?;
            if tokens.len() >= dom_idx + 3 && tokens[dom_idx] == "device" {
                if let Some(dev_id) = first_positional_after(&tokens, dom_idx + 2) {
                    return Some(dev_id);
                }
            }
        }
    }
    None
}

/// Extract a single entity candidate from a `neomind <domain> <verb> ...` shell
/// invocation. Skips pipelines (`|`), chained commands (`&&`, `;`), and `list`
/// commands so exploratory or ambiguous invocations never spam cards.
fn extract_from_shell_command(
    cmd: &str,
    result: &str,
) -> Option<(
    ChatAttachmentKind,
    ChatAttachmentAction,
    String,
    Option<String>,
    Option<String>,
)> {
    let trimmed = cmd.trim();
    // Conservative guard: skip compound shell pipelines/chains (only outside quotes).
    if has_unquoted_shell_operator(trimmed) {
        return None;
    }

    let tokens = tokenize_simple_command(trimmed)?;
    let dom_idx = neomind_domain_index(&tokens)?;
    if tokens.len() < dom_idx + 2 {
        return None;
    }

    let kind = match tokens[dom_idx].as_str() {
        "device" => ChatAttachmentKind::Device,
        "rule" => ChatAttachmentKind::Rule,
        "transform" => ChatAttachmentKind::Transform,
        "agent" => ChatAttachmentKind::Agent,
        "dashboard" => ChatAttachmentKind::Dashboard,
        _ => return None,
    };

    let verb = tokens[dom_idx + 1].as_str();
    let args_start = dom_idx + 2;
    let action = match verb {
        // List queries never expand into individual entity cards.
        "list" | "types" | "virtual-metrics" | "data-sources" | "plan-preview" | "drafts" => {
            return None
        }
        "get" | "latest" | "metrics" | "history" | "telemetry" | "latest-execution"
        | "executions" | "explain" | "memory" | "conversation" | "status" => {
            ChatAttachmentAction::Queried
        }
        "create" | "add" => ChatAttachmentAction::Created,
        "update" | "enable" | "disable" | "add-components" | "update-component"
        | "remove-components" => ChatAttachmentAction::Updated,
        "control" => {
            if kind == ChatAttachmentKind::Device {
                ChatAttachmentAction::Executed
            } else {
                ChatAttachmentAction::Updated
            }
        }
        "send-command" | "command" | "invoke" | "test" | "execute" | "send-message"
        | "write-metric" | "share" => ChatAttachmentAction::Executed,
        "delete" | "remove" => ChatAttachmentAction::Deleted,
        _ => return None,
    };

    // Parse shell stdout (JSON or human key-value).
    let stdout = extract_shell_stdout(result);
    let parsed_json = stdout
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s.trim()).ok());

    let id_from_json = parsed_json
        .as_ref()
        .and_then(|v| extract_entity_id_from_json(kind, v));
    let id_from_human = stdout
        .as_deref()
        .and_then(|s| extract_entity_id_from_human(kind, s));
    let id_from_args = first_positional_after(&tokens, args_start);

    let entity_id = match action {
        // For create/add, the new ID only exists in stdout.
        ChatAttachmentAction::Created => id_from_json.or(id_from_human)?,
        // Prefer canonical UUID from JSON response over CLI positional name/alias.
        _ => id_from_json.or(id_from_args).or(id_from_human)?,
    };

    let fallback_title = parsed_json
        .as_ref()
        .and_then(extract_name_from_json);

    let extra_subtitle = if kind == ChatAttachmentKind::Device
        && matches!(verb, "control" | "send-command" | "command")
    {
        // Positional arg after device_id is the command name.
        nth_positional_after(&tokens, args_start, 1).map(|c| format!("command: {c}"))
    } else {
        None
    };

    Some((kind, action, entity_id, fallback_title, extra_subtitle))
}

fn extract_from_domain_tool(
    tool: &str,
    args: &Value,
    result: &str,
) -> Option<(ChatAttachmentKind, ChatAttachmentAction, String, Option<String>)> {
    let kind = match tool {
        "device" => ChatAttachmentKind::Device,
        "rule" => ChatAttachmentKind::Rule,
        "transform" => ChatAttachmentKind::Transform,
        "agent" => ChatAttachmentKind::Agent,
        _ => return None,
    };
    let verb = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let action = match verb {
        "list" => return None,
        "get" | "latest" | "history" | "status" => ChatAttachmentAction::Queried,
        "create" => ChatAttachmentAction::Created,
        "update" | "enable" | "disable" => ChatAttachmentAction::Updated,
        "control" | "send_command" | "invoke" | "test" | "execute" => {
            ChatAttachmentAction::Executed
        }
        "delete" => ChatAttachmentAction::Deleted,
        _ => return None,
    };
    let parsed = serde_json::from_str::<Value>(result).ok();
    let id_from_args = args
        .get("id")
        .or_else(|| args.get("device_id"))
        .or_else(|| args.get("rule_id"))
        .or_else(|| args.get("transform_id"))
        .or_else(|| args.get("agent_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let id_from_res = parsed
        .as_ref()
        .and_then(|v| extract_entity_id_from_json(kind, v));
    let entity_id = id_from_res.or(id_from_args)?;
    let title = parsed.as_ref().and_then(extract_name_from_json);
    Some((kind, action, entity_id, title))
}

fn extract_shell_stdout(result: &str) -> Option<String> {
    if let Ok(val) = serde_json::from_str::<Value>(result) {
        if let Some(s) = val.get("stdout").and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
    }
    Some(result.to_string())
}

fn extract_entity_id_from_json(kind: ChatAttachmentKind, val: &Value) -> Option<String> {
    let target = val.get("data").unwrap_or(val);
    let keys: &[&str] = match kind {
        ChatAttachmentKind::Device => &["device_id", "deviceId", "id"],
        ChatAttachmentKind::Rule => &["rule_id", "ruleId", "id"],
        ChatAttachmentKind::Transform => &["transform_id", "transformId", "id"],
        ChatAttachmentKind::Agent => &["agent_id", "agentId", "id"],
        ChatAttachmentKind::Dashboard => &["dashboard_id", "dashboardId", "id"],
        ChatAttachmentKind::Image => return None,
    };
    for k in keys {
        if let Some(s) = target.get(*k).and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    // Nested object e.g. { data: { device: { id: ... } } }, { data: { automation: { id: ... } } },
    // or { data: { execution: { agent_id: ... } } }.
    let nested_keys: &[&str] = match kind {
        ChatAttachmentKind::Transform => &["transform", "automation"],
        ChatAttachmentKind::Agent => &["agent", "execution"],
        _ => &[kind.as_str()],
    };
    for nk in nested_keys {
        if let Some(nested) = target.get(*nk) {
            for k in keys {
                if let Some(s) = nested.get(*k).and_then(|v| v.as_str()) {
                    let t = s.trim();
                    if !t.is_empty() {
                        return Some(t.to_string());
                    }
                }
            }
            if let Some(meta_id) = nested
                .get("metadata")
                .and_then(|m| m.get("id"))
                .and_then(|v| v.as_str())
            {
                let t = meta_id.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

fn extract_name_from_json(val: &Value) -> Option<String> {
    let target = val.get("data").unwrap_or(val);
    target
        .get("name")
        .or_else(|| target.get("title"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn extract_entity_id_from_human(kind: ChatAttachmentKind, stdout: &str) -> Option<String> {
    let prefixes: &[&str] = match kind {
        ChatAttachmentKind::Device => &["device_id:", "id:"],
        ChatAttachmentKind::Rule => &["rule_id:", "id:"],
        ChatAttachmentKind::Transform => &["transform_id:", "id:"],
        ChatAttachmentKind::Agent => &["agent_id:", "id:"],
        ChatAttachmentKind::Dashboard => &["dashboard_id:", "id:"],
        ChatAttachmentKind::Image => return None,
    };
    for line in stdout.lines().take(24) {
        let trimmed = line.trim();
        for p in prefixes {
            if let Some(rest) = trimmed.strip_prefix(p) {
                let id = rest.trim().trim_matches('"').trim_matches('\'');
                if !id.is_empty() && !id.contains(' ') {
                    return Some(id.to_string());
                }
            }
        }
    }
    None
}

/// Returns true if `cmd` contains unquoted shell chaining/pipe operators (`|`, `&&`, `||`, `;`).
fn has_unquoted_shell_operator(cmd: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == '\\' && q == '"' {
                    chars.next();
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                '\\' => {
                    chars.next();
                }
                '|' | ';' => return true,
                '&' if matches!(chars.peek(), Some('&')) => return true,
                _ => {}
            },
        }
    }
    false
}

/// Simple shell tokenizer handling single/double quotes and POSIX line
/// continuations (sufficient for `neomind <domain> <verb> <id> --flag val`).
fn tokenize_simple_command(cmd: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = cmd.chars().peekable();

    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else if c == '\\' && q == '"' {
                    if let Some(next) = chars.next() {
                        if next == '\n' {
                            // POSIX line continuation inside double quotes
                        } else if next == '\r' && matches!(chars.peek(), Some('\n')) {
                            chars.next();
                        } else {
                            cur.push(next);
                        }
                    }
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                '\\' => {
                    if let Some(next) = chars.next() {
                        if next == '\n' {
                            // POSIX line continuation
                        } else if next == '\r' && matches!(chars.peek(), Some('\n')) {
                            chars.next();
                        } else {
                            cur.push(next);
                        }
                    }
                }
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        tokens.push(std::mem::take(&mut cur));
                    }
                }
                _ => cur.push(c),
            },
        }
    }
    if quote.is_some() {
        return None;
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    Some(tokens)
}

fn first_positional_after(tokens: &[String], start: usize) -> Option<String> {
    nth_positional_after(tokens, start, 0)
}

fn nth_positional_after(tokens: &[String], start: usize, target_idx: usize) -> Option<String> {
    let mut i = start;
    let mut pos_idx = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.starts_with("--") {
            // Flags with separate value vs boolean flags.
            let next_looks_like_json = tokens
                .get(i + 1)
                .is_some_and(|n| n.trim_start().starts_with('{') || n.trim_start().starts_with('['));
            let is_bool = match tok.as_str() {
                "--json" => !next_looks_like_json,
                "--yes"
                | "--force"
                | "--human"
                | "--enabled"
                | "--disabled"
                | "--all"
                | "--dry-run"
                | "--no-tools"
                | "--no-auto-tools"
                | "--verbose"
                | "--quiet" => true,
                _ => false,
            };
            if !tok.contains('=') && !is_bool {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if tok.starts_with('-') && tok.len() == 2 {
            // Note: `-f` is `--format <fmt>` (value-taking), NOT boolean.
            let is_bool = matches!(tok.as_str(), "-y" | "-j" | "-a" | "-v");
            i += if is_bool { 1 } else { 2 };
            continue;
        }
        if pos_idx == target_idx {
            return Some(tok.clone());
        }
        pos_idx += 1;
        i += 1;
    }
    None
}

/// Extract safe `/api/images/...` paths from tool result text.
pub fn extract_image_urls(raw: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut seen = HashSet::new();
    let needle = "/api/images/";
    let mut offset = 0;

    while let Some(rel_idx) = raw[offset..].find(needle) {
        let start = offset + rel_idx;
        let rest = &raw[start..];
        let end = rest
            .find(|c: char| {
                c.is_whitespace()
                    || c.is_control()
                    || matches!(
                        c,
                        '"' | '\'' | '`' | ')' | '(' | ']' | '[' | '}' | '{' | '<' | '>' | ',' | ';' | '\\' | '?' | '#'
                    )
            })
            .unwrap_or(rest.len());
        let candidate = rest[..end].trim_end_matches('.');
        if is_valid_image_url(candidate) && seen.insert(candidate.to_string()) {
            urls.push(candidate.to_string());
        }
        offset = start + end.max(needle.len());
    }
    urls
}

fn is_valid_image_url(url: &str) -> bool {
    let Some(tail) = url.strip_prefix("/api/images/") else {
        return false;
    };
    if tail.is_empty() || tail.contains("..") || tail.starts_with('.') {
        return false;
    }
    let lower = tail.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Extract `/api/images/...` URLs already referenced as markdown images
/// `![...](/api/images/...)` in the assistant's reply so we never show
/// duplicate media below the message.
pub fn extract_markdown_image_urls(markdown: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut offset = 0;
    while let Some(rel) = markdown[offset..].find("![") {
        let start = offset + rel;
        let rest = &markdown[start..];
        let Some(mid) = rest.find("](") else {
            break;
        };
        let after = &rest[mid + 2..];
        let Some(close) = after.find(')') else {
            break;
        };
        let raw_target = after[..close].trim();
        // Strip optional title or angle brackets
        let target = raw_target
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches(|c| c == '<' || c == '>');
        if let Some(pos) = target.find("/api/images/") {
            let path_with_query = &target[pos..];
            let path = path_with_query
                .split(['?', '#'])
                .next()
                .unwrap_or(path_with_query);
            if is_valid_image_url(path) {
                out.insert(path.to_string());
            }
        }
        offset = start + mid + 2 + close + 1;
    }
    out
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collector_merges_same_entity_and_upgrades_queried_to_updated() {
        let mut c = TurnAttachmentCollector::new();
        c.record_tool_start(
            "shell",
            &json!({"command": "neomind device get dev-01 --json"}),
        );
        c.record_tool_end(
            "shell",
            &json!({"exit_code": 0, "stdout": "{\"success\":true,\"data\":{\"device_id\":\"dev-01\",\"name\":\"Sensor\"}}"}).to_string(),
            true,
        );

        c.record_tool_start(
            "shell",
            &json!({"command": "neomind device send-command dev-01 reboot --json"}),
        );
        c.record_tool_end(
            "shell",
            &json!({"exit_code": 0, "stdout": "{\"success\":true,\"data\":{\"sent\":true}}"}).to_string(),
            true,
        );

        assert_eq!(c.entities.len(), 1);
        let entry = c.entities.get("device:dev-01").unwrap();
        assert_eq!(entry.action, ChatAttachmentAction::Executed);
        assert_eq!(
            entry.action.default_display(entry.kind),
            ChatAttachmentDisplay::Card
        );
        assert_eq!(entry.extra_subtitle.as_deref(), Some("command: reboot"));
    }

    #[test]
    fn list_and_failed_commands_are_ignored() {
        let mut c = TurnAttachmentCollector::new();
        c.record_tool_start("shell", &json!({"command": "neomind device list --json"}));
        c.record_tool_end(
            "shell",
            &json!({"exit_code": 0, "stdout": "{\"success\":true,\"data\":[{\"id\":\"d1\",\"image\":\"/api/images/cam-1/image/1.jpg\"}]}"})
                .to_string(),
            true,
        );

        c.record_tool_start(
            "shell",
            &json!({"command": "neomind rule create --json '{}'" }),
        );
        c.record_tool_end(
            "shell",
            &json!({"exit_code": 1, "stderr": "invalid"}).to_string(),
            true,
        );

        // Chained shell pipelines are conservatively skipped.
        c.record_tool_start(
            "shell",
            &json!({"command": "neomind device get dev-01 | jq ."}),
        );
        c.record_tool_end("shell", &json!({"exit_code": 0, "stdout": "{}"}).to_string(), true);

        assert!(c.is_empty());
    }

    #[test]
    fn create_extracts_id_from_json_and_human_stdout() {
        let mut c = TurnAttachmentCollector::new();
        c.record_tool_start("shell", &json!({"command": "neomind rule create --json '{}'"}));
        c.record_tool_end(
            "shell",
            &json!({
                "exit_code": 0,
                "stdout": "{\"success\":true,\"data\":{\"id\":\"rule-99\",\"name\":\"High Temp\"}}"
            })
            .to_string(),
            true,
        );
        let rule = c.entities.get("rule:rule-99").unwrap();
        assert_eq!(rule.action, ChatAttachmentAction::Created);
        assert_eq!(rule.fallback_title.as_deref(), Some("High Temp"));

        c.record_tool_start(
            "shell",
            &json!({"command": "neomind transform create --name Parse"}),
        );
        c.record_tool_end(
            "shell",
            &json!({
                "exit_code": 0,
                "stdout": "✅ Created transform\n  id: tf-42\n  name: Parse\n"
            })
            .to_string(),
            true,
        );
        assert!(c.entities.contains_key("transform:tf-42"));
    }

    #[test]
    fn image_urls_extract_and_dedupe_against_markdown() {
        let raw = r#"{"url": "/api/images/cam-1/image/170000.jpg", "cjk": "/api/images/前门相机/image/170001.png", "bad": "/api/images/../secret.png"}"#;
        let found = extract_image_urls(raw);
        assert_eq!(
            found,
            vec![
                "/api/images/cam-1/image/170000.jpg",
                "/api/images/前门相机/image/170001.png"
            ]
        );

        let md = "Here is the snapshot:\n![front](/api/images/cam-1/image/170000.jpg)\nDone.";
        let inlined = extract_markdown_image_urls(md);
        assert!(inlined.contains("/api/images/cam-1/image/170000.jpg"));
    }

    #[test]
    fn leading_global_flags_and_sent_false_and_unsafe_ids() {
        let mut c = TurnAttachmentCollector::new();
        // Leading global `--json` flag before domain should still parse.
        c.record_tool_start(
            "shell",
            &json!({"command": "neomind --json device get dev-07"}),
        );
        c.record_tool_end(
            "shell",
            &json!({"exit_code": 0, "stdout": "{\"success\":true,\"data\":{\"device_id\":\"dev-07\"}}"}).to_string(),
            true,
        );
        assert!(c.entities.contains_key("device:dev-07"));

        // `sent: false` must be treated as business failure (no executed card).
        c.record_tool_start(
            "shell",
            &json!({"command": "neomind device send-command dev-08 reboot"}),
        );
        c.record_tool_end(
            "shell",
            &json!({"exit_code": 0, "stdout": "{\"success\":true,\"data\":{\"sent\":false}}"}).to_string(),
            true,
        );
        assert!(!c.entities.contains_key("device:dev-08"));

        // Unsafe traversal IDs are rejected.
        c.record_tool_start(
            "shell",
            &json!({"command": "neomind device get ../secret"}),
        );
        c.record_tool_end(
            "shell",
            &json!({"exit_code": 0, "stdout": "{\"success\":true}"}).to_string(),
            true,
        );
        assert_eq!(c.entities.len(), 1);
    }

    #[test]
    fn dashboard_shell_commands_extract_cards_and_skip_list() {
        let mut dc = TurnAttachmentCollector::new();
        dc.record_tool_start(
            "shell",
            &json!({"command": r#"neomind dashboard create --name "温室监控""#}),
        );
        dc.record_tool_end(
            "shell",
            &json!({"exit_code": 0, "stdout": "{\"success\":true,\"data\":{\"id\":\"dashboard_abc\",\"name\":\"温室监控\"}}"}).to_string(),
            true,
        );
        let dash = dc
            .entities
            .get("dashboard:dashboard_abc")
            .expect("dashboard create");
        assert_eq!(dash.action, ChatAttachmentAction::Created);

        // add-components on same turn preserves higher Created priority
        dc.record_tool_start(
            "shell",
            &json!({"command": r#"neomind dashboard add-components dashboard_abc --components '[{"type":"value-card"}]'"#}),
        );
        dc.record_tool_end(
            "shell",
            &json!({"exit_code": 0, "stdout": "{\"success\":true,\"data\":{\"id\":\"dashboard_abc\",\"components\":[{\"id\":\"c1\"}]}}"}).to_string(),
            true,
        );
        assert_eq!(
            dc.entities.get("dashboard:dashboard_abc").unwrap().action,
            ChatAttachmentAction::Created
        );

        // dashboard list must stay ignored
        let mut lc = TurnAttachmentCollector::new();
        lc.record_tool_start("shell", &json!({"command": "neomind dashboard list"}));
        lc.record_tool_end(
            "shell",
            &json!({"exit_code": 0, "stdout": "{\"success\":true,\"data\":{\"dashboards\":[{\"id\":\"d1\"}]}}"}).to_string(),
            true,
        );
        assert!(lc.is_empty());
    }
}
