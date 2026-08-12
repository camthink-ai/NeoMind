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
        let state = self.build_system_state_snapshot().await;
        if !state.is_empty() {
            out.push_str(&state);
        }
        tracing::debug!(bytes = out.len(), "capability index built");
        out
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
- Boolean flags take a value with `=`: `--enabled=true`, `--tls=false`, `--compress=true`, `--public=false` (NOT bare `--enabled`). Applies across rule/agent/transform/push/connector/device enable·tls·compress·public·auto_approve.
- device history: `neomind device history <ID> --metric <field> --time-range <range> [--compress=true]`
  - `--time-range`: `1h`, `24h`, `7d`, `30d` (there is no --start/--end)
  - `--metric`: a field name from device list `metric_fields` (e.g. `values.battery`, `temperature`)
  - `--compress=true` for an AI-friendly compact series; image metrics are auto-summarized regardless
- device get <id> [--metric <field>] — returns current metrics for one device (data.metrics.{name}.{value,unit,timestamp}). Pass `--metric values.battery` to get only that field's current value (avoids pulling all metrics, e.g. AI-camera inference fields); omit for all. For a metric's time series use `device history <id> --metric <field>`. For webhook devices, get the ingest URL with `device webhook-url <id>` (only for webhook adapter devices).
- Image metrics (e.g. ne101 `values.image`, ne301 `image_data`): `device get <id>` returns image values as `/api/images/...` URLs. Pass the URL directly to `vision(image="/api/images/...", prompt=...)`. Do NOT pipe to python or save-to-file — the URL works as-is. If vision can't see the image, pass `model="minicpm-v4.6"` to force the right VLM.
- Output is structured JSON (NEOMIND_JSON=1); errors carry a `suggestion` field with recovery hints
- The shell tool runs neomind in-process — no shell pipes/redirects (`|`, `>`, `2>&1`, `head`). Filter/limit with command flags (`--device-type`, `--status`, `--limit`), not shell plumbing.
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
            "must warn the shell tool has no pipes/redirects"
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
        // No skill list duplication
        assert!(!out.contains("device-onboarding"));
        // ~1.2K token target ≈ well under 6KB chars
        assert!(
            out.len() <= 6000,
            "capability index too large: {} bytes",
            out.len()
        );
    }
}
