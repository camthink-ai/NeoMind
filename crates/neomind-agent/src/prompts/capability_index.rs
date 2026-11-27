//! System capability index: a resident "capability map" injected into the
//! agent's dynamic system prompt so the LLM stops doing pure-exploration
//! calls (--help, repeated skill load, field-name guessing) at task start.
//!
//! Three parts: auto-generated CLI command tree (from clap), data conventions
//! (hardcoded), and a device-type snapshot (from the shared ResourceIndex).
//! Skill IDs are intentionally NOT listed here — already visible via the
//! `skill` tool description (pasted into "Available Tools").

use std::collections::HashMap;
use std::sync::Arc;

use clap::CommandFactory;
use tokio::sync::RwLock;

#[cfg(test)]
use crate::context::Resource;
use crate::context::ResourceIndex;
use neomind_cli_ops::dispatch::commands::Args;

/// Resident system-capability map builder. Shares the same `ResourceIndex`
/// as `SemanticToolMapper` (zero new service wiring).
#[derive(Clone)]
pub struct CapabilityIndex {
    resource_index: Arc<RwLock<ResourceIndex>>,
}

impl CapabilityIndex {
    pub fn new(resource_index: Arc<RwLock<ResourceIndex>>) -> Self {
        Self { resource_index }
    }

    /// Build the full capability index string (~1–1.2K tokens target).
    pub async fn build(&self) -> String {
        let mut out = String::from("## System Capability Index\n");
        out.push_str(&Self::build_cli_tree());
        out.push_str(&Self::build_data_conventions());
        // NM-252: fixed discovery rules (same length for 5 or 500 installs).
        // Prefer --search then invoke; reuse known on follow-up. Transitional
        // NM-247 catalog remains available but is not injected here.
        out.push_str(&neomind_core::extension::format_extension_discovery_rules());
        let state = self.build_system_state_snapshot().await;
        if !state.is_empty() {
            out.push_str(&state);
        }
        out.push_str(&Self::boundary_rules());
        tracing::debug!(bytes = out.len(), "capability index built");
        out
    }

    /// Boundary rules (negative capability map). Hot-updatable without a
    /// rebuild: set `NEOMIND_BOUNDARY_RULES` to a file path whose contents
    /// are appended verbatim under a `## Boundary Rules` heading. When the
    /// env var is unset (the default) nothing is injected — the shipped
    /// prompt is unchanged.
    pub fn boundary_rules() -> String {
        match std::env::var("NEOMIND_BOUNDARY_RULES") {
            Ok(path) if !path.is_empty() => match std::fs::read_to_string(&path) {
                Ok(rules) if !rules.trim().is_empty() => format!("\n{}", rules.trim()),
                Ok(_) => String::new(),
                Err(e) => {
                    tracing::warn!(path = %path, error = %e, "NEOMIND_BOUNDARY_RULES unreadable");
                    String::new()
                }
            },
            _ => String::new(),
        }
    }

    /// CLI command tree skeleton, auto-generated from clap. Filters to the
    /// 14 domains that have subcommands; leaf commands (Serve/Prompt/Chat/
    /// Health/Logs/ListModels/CheckUpdate/Login/Logout/Whoami) are
    /// interactive/local or self-evident and are excluded.
    pub fn build_cli_tree() -> String {
        let mut s = String::from("\n### CLI Commands\n");
        let cmd = Args::command();
        for sub in cmd.get_subcommands() {
            if !sub.has_subcommands() {
                continue; // skip leaf commands
            }
            let name = sub.get_name();
            let about = sub.get_about().map(|x| x.to_string()).unwrap_or_default();
            let sub_names: Vec<&str> = sub.get_subcommands().map(|c| c.get_name()).collect();
            s.push_str(&format!(
                "- {}: {} — {}\n",
                name,
                sub_names.join("/"),
                about
            ));
        }
        s
    }

    /// Stable data conventions (hardcoded). Updated only on API-level changes.
    pub fn build_data_conventions() -> String {
        r###"
### Data Conventions
- Run `neomind device list` first for the overview — read `summary`, `types[].type`, `types[].metric_fields` (field names per type), and `types[].devices.list` (id/name/status). Filter with `--device-type <type>` (not `--type`) or `--status`. Fetch a device's current values with `device get <id>`, history with `device history <id> --metric <field> --time-range 7d`.
- Boolean flags take a value with `=`: `--enabled=true`, `--tls=false`, `--compress=true`, `--public=false` (NOT bare `--enabled`). Applies across rule/agent/transform/push/connector/device enable·tls·compress·public.
- device history: `neomind device history <ID> --metric <field> --time-range <range> [--compress=true]`
  - `--time-range`: `1h`, `24h`, `7d`, `30d` (there is no --start/--end)
  - `--metric`: a field name from device list `metric_fields` (e.g. `values.battery`, `temperature`)
  - `--compress=true` for an AI-friendly compact series; image metrics are auto-summarized regardless
- device get <id> [--metric <field>] [--commands] — returns device metadata, available commands (`data.commands` with `params_example`/`shell_example`), and current metrics (`data.metrics.{name}.{value,unit,timestamp}`). Pass `--commands` before `device control` for a compact commands-only view (omits metrics). Pass `--metric values.battery` to get only that field's current value (avoids pulling all metrics, e.g. AI-camera inference fields); omit for all. For a metric's time series use `device history <id> --metric <field>`. For webhook devices, get the ingest URL with `device webhook-url <id>` (only for webhook adapter devices).
- Image metrics (e.g. ne101 `values.image`, ne301 `image_data`): `device get <id>` returns image values as `/api/images/...` URLs. Pass the URL directly to `vision(image="/api/images/...", prompt=...)`. Do NOT pipe to python or save-to-file — the URL works as-is.
- Output is structured JSON (NEOMIND_JSON=1); errors carry a `suggestion` field with recovery hints
- `neomind` commands run in-process with no shell pipes/redirects (`|`, `>`, `2>&1`) — filter/limit with built-in flags (`--device-type`, `--status`, `--limit`) instead of `| grep/jq`. Only native host commands support shell pipes and redirections.
- For create/update commands with many unfamiliar fields, `skill load` the matching skill (rule-management, agent-management, etc.) — field schemas live there. For simple create/update with known fields, run the CLI directly.
"###
            .to_string()
    }

    /// Device-type × count snapshot, aggregated from the shared ResourceIndex.
    /// Empty string when no devices (caller omits the section).
    pub async fn build_system_state_snapshot(&self) -> String {
        let devices = {
            let idx = self.resource_index.read().await;
            idx.list_devices().await
        };
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for r in &devices {
            if let Some(d) = r.as_device() {
                *counts.entry(d.device_type.as_str()).or_insert(0) += 1;
            }
        }
        if counts.is_empty() {
            return String::new();
        }
        let total: usize = counts.values().sum();
        let mut entries: Vec<(&str, usize)> = counts.into_iter().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let mut s = String::from("\n### System State\n");
        s.push_str(&format!("- devices: {} total\n", total));
        for (t, c) in entries {
            s.push_str(&format!("  - {}: {}\n", t, c));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_conventions_mentions_key_rules() {
        let c = CapabilityIndex::build_data_conventions();
        assert!(c.contains("metric_fields"));
        assert!(c.contains("--time-range"), "must mention --time-range");
        assert!(
            c.contains("--compress=true"),
            "must mention the --compress=true form"
        );
        assert!(
            c.contains("--enabled=true"),
            "must state the generic bool=value rule"
        );
        assert!(c.contains("NEOMIND_JSON"));
        assert!(c.contains("suggestion"));
        assert!(
            c.contains("--device-type"),
            "must mention --device-type filter (not --type)"
        );
        assert!(
            c.contains("no shell pipes"),
            "must warn neomind in-process commands have no shell pipes/redirects"
        );
        assert!(
            c.contains("native host commands support shell pipes"),
            "must clarify native host commands still support pipes"
        );
        assert!(
            !c.contains("minicpm-v4.6"),
            "must not hardcode a specific VLM model name"
        );
        assert!(
            c.contains("devices.list"),
            "must mention devices.list for the device roster"
        );
        assert!(
            c.contains("/api/images/"),
            "must teach the /api/images/ URL flow (device get -> vision)"
        );
        assert!(
            c.contains("device get <id> [--metric"),
            "must document device get optional --metric filter"
        );
        assert!(
            c.contains("--commands"),
            "must document device get --commands option"
        );
    }

    #[test]
    fn cli_tree_has_14_domains_and_device_history() {
        let tree = CapabilityIndex::build_cli_tree();
        let domains = [
            "device",
            "dashboard",
            "rule",
            "agent",
            "extension",
            "llm",
            "message",
            "transform",
            "widget",
            "push",
            "connector",
            "settings",
            "system",
            "api-key",
        ];
        for d in domains {
            assert!(tree.contains(&format!("- {}:", d)), "missing domain {}", d);
        }
        assert!(
            tree.contains("history"),
            "device subcommands should include history"
        );
        assert!(
            !tree.contains("- serve:"),
            "leaf command serve must be excluded"
        );
        assert!(
            !tree.contains("- chat:"),
            "leaf command chat must be excluded"
        );
        assert!(
            !tree.contains("- health:"),
            "leaf command health must be excluded"
        );
    }

    #[tokio::test]
    async fn snapshot_empty_for_empty_index() {
        let idx = Arc::new(RwLock::new(ResourceIndex::new()));
        let ci = CapabilityIndex::new(idx);
        let s = ci.build_system_state_snapshot().await;
        assert!(s.is_empty(), "empty index must yield empty snapshot");
    }

    #[tokio::test]
    async fn snapshot_aggregates_by_device_type() {
        let idx = make_index_with_devices(&[
            ("d1", "Water Meter", "ne101_camera"),
            ("d2", "Car Park", "ne101_camera"),
            ("d3", "NE302", "ne301_camera"),
        ])
        .await;
        let ci = CapabilityIndex::new(Arc::new(RwLock::new(idx)));
        let s = ci.build_system_state_snapshot().await;
        assert!(s.contains("3 total"), "total count: {}", s);
        assert!(
            s.contains("ne101_camera") && s.contains("ne301_camera"),
            "{}",
            s
        );
    }

    // Uses Resource::device(id, name, type) (resource_index.rs:705) + ResourceIndex::register
    // (resource_index.rs:271, pub async) — both verified public.
    async fn make_index_with_devices(specs: &[(&str, &str, &str)]) -> ResourceIndex {
        let idx = ResourceIndex::new();
        for (id, name, ty) in specs {
            let r = Resource::device((*id).to_string(), (*name).to_string(), (*ty).to_string());
            idx.register(r).await.expect("register");
        }
        idx
    }

    #[tokio::test]
    async fn build_has_all_sections_and_stays_compact() {
        let ci = CapabilityIndex::new(Arc::new(RwLock::new(ResourceIndex::new())));
        let out = ci.build().await;
        assert!(out.starts_with("## System Capability Index"));
        assert!(out.contains("### CLI Commands"));
        assert!(out.contains("### Data Conventions"));
        assert!(
            out.contains("### Extension capability discovery"),
            "NM-252 discovery rules must be resident: {out}"
        );
        assert!(
            out.contains("--search"),
            "must prefer commands --search for concrete tasks: {out}"
        );
        assert!(
            out.contains("extension list"),
            "overview must route to extension list: {out}"
        );
        assert!(
            out.contains("shell_example") || out.contains("{command, params}"),
            "must teach shell_example paste: {out}"
        );
        // No skill list duplication
        assert!(!out.contains("device-onboarding"));
        // Fixed discovery rules — must not grow with installs (no per-ext catalog).
        assert!(
            !out.contains("### AI Extensions"),
            "transitional NM-247 catalog must not be injected: {out}"
        );
        // ~1.2K token target ≈ well under 6KB chars
        assert!(
            out.len() <= 6000,
            "capability index too large: {} bytes",
            out.len()
        );
    }

    /// Anti-drift guard 1: every entry in `CORE_CONTRACTS` must reference a real
    /// clap `(domain, action)` subcommand, and its `primary_json.flag` (if any)
    /// must be an actual long flag declared on that subcommand.
    #[test]
    fn core_contracts_match_clap_cli_tree() {
        let root = Args::command();
        let mut problems = Vec::new();

        for contract in neomind_core::command_contract::CORE_DOMAIN_CONTRACTS {
            let Some(dom) = root.find_subcommand(contract.domain) else {
                problems.push(format!(
                    "contract ({}, {}): domain '{}' not found in clap Args",
                    contract.domain, contract.action, contract.domain
                ));
                continue;
            };
            let Some(act) = dom.find_subcommand(contract.action) else {
                problems.push(format!(
                    "contract ({}, {}): action '{}' not found under '{}'",
                    contract.domain, contract.action, contract.action, contract.domain
                ));
                continue;
            };
            if let Some(spec) = &contract.primary_json {
                let expected = spec.flag.trim_start_matches("--");
                let has_flag = act.get_arguments().any(|arg| {
                    arg.get_long() == Some(expected)
                        || arg
                            .get_all_aliases()
                            .is_some_and(|al| al.contains(&expected))
                });
                if !has_flag {
                    problems.push(format!(
                        "contract ({}, {}): primary_json flag '{}' not declared on clap subcommand",
                        contract.domain, contract.action, spec.flag
                    ));
                }
            }
        }

        assert!(
            problems.is_empty(),
            "CORE_CONTRACTS drifted from clap CLI:\n  {}",
            problems.join("\n  ")
        );
    }

    /// Anti-drift guard 2: every `neomind <domain> <action>` command cited in the
    /// 15 builtin skills (excluding explicit negation examples like "NOT" / "do NOT exist")
    /// must resolve to a real clap subcommand or alias.
    #[test]
    fn builtin_skills_cli_commands_match_clap_cli_tree() {
        let root = Args::command();
        let mut valid_actions: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
        for dom in root.get_subcommands() {
            if !dom.has_subcommands() {
                continue;
            }
            let mut set = std::collections::HashSet::new();
            for sub in dom.get_subcommands() {
                set.insert(sub.get_name().to_string());
                for alias in sub.get_all_aliases() {
                    set.insert(alias.to_string());
                }
            }
            valid_actions.insert(dom.get_name().to_string(), set);
        }

        let registry = crate::skills::SkillRegistry::load_all(None);

        let mut problems = Vec::new();
        for skill in registry.list() {
            let mut in_fence = false;
            for (line_idx, line) in skill.body.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("```") {
                    in_fence = !in_fence;
                    continue;
                }
                // Skip lines explicitly illustrating non-existent commands
                if line.contains("do NOT exist")
                    || line.contains("NOT ")
                    || line.contains("wrong")
                    || line.contains("Wrong")
                {
                    continue;
                }
                // Collect CLI command snippets: either full lines inside fenced
                // code blocks, or inline backtick spans (`neomind ...`).
                let mut snippets: Vec<&str> = Vec::new();
                if in_fence {
                    if let Some(rest) = trimmed.strip_prefix("neomind ") {
                        snippets.push(rest);
                    }
                } else {
                    let mut rest = line;
                    while let Some(start) = rest.find('`') {
                        let after = &rest[start + 1..];
                        let Some(end) = after.find('`') else { break };
                        let span = after[..end].trim();
                        if let Some(cmd) = span.strip_prefix("neomind ") {
                            snippets.push(cmd);
                        }
                        rest = &after[end + 1..];
                    }
                }
                for snippet in snippets {
                    let parts: Vec<&str> = snippet.split_whitespace().collect();
                    if parts.len() < 2 {
                        continue;
                    }
                    let (dom, act) = (parts[0], parts[1]);
                    if act.starts_with('-')
                        || act.starts_with('<')
                        || act.starts_with('[')
                        || act.contains('{')
                        || act.contains('|')
                    {
                        continue;
                    }
                    if let Some(allowed) = valid_actions.get(dom) {
                        if !allowed.contains(act) {
                            problems.push(format!(
                                "skill '{}' L{}: unknown CLI command `neomind {} {}`",
                                skill.metadata.id,
                                line_idx + 1,
                                dom,
                                act
                            ));
                        }
                    }
                }
            }
        }

        assert!(
            problems.is_empty(),
            "builtin skills cite non-existent CLI subcommands:\n  {}",
            problems.join("\n  ")
        );
    }
}
