//! Spawn + health-poll the bundled llama-server process.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LlamaServerConfig {
    pub binary: PathBuf,
    pub model: PathBuf,
    pub port: u16,
    pub ctx: usize,
    pub ngl: Option<u16>,
    pub threads: Option<usize>,
    /// Per-model sampling defaults (official/model-card tuned). Passed as
    /// server-side defaults (--temp/--top-p/--top-k): any request that omits
    /// sampling gets the model's best-known point instead of the llama.cpp
    /// generic default.
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    /// min_p: MiniCPM5's official card REQUIRES 0.0 — llama.cpp's 0.05
    /// default truncates the tail the model was trained with and causes
    /// repetition loops (OpenBMB issue #374).
    pub min_p: Option<f32>,
    /// repeat_penalty: part of some official cards (LFM 1.1, MiniCPM 1.05
    /// for anti-repetition); llama.cpp's default is 1.0 (off).
    pub repeat_penalty: Option<f32>,
    /// KV cache q8_0 + flash attention. On by default (upstream practice —
    /// q8 KV is near-lossless and roughly halves KV memory; verified on an
    /// NG4520: 32K window with q8 KV uses LESS device memory than 16K with
    /// f16). Disable with NEOMIND_BUILTIN_LLM_KV=f16 for old runtimes or
    /// architectures where FA/q8 KV misbehaves.
    pub kv_quant: bool,
    /// llama-server slot count. `Some(1)` (the default) keeps the
    /// platform's prompt budget exact — a multi-slot server reports
    /// `total_slots>1` and the platform budgets n_ctx ÷ slots, silently
    /// shrinking the usable window. `None` omits the flag entirely
    /// (upstream default: 4 slots, unified KV).
    pub parallel: Option<usize>,
    /// Vision projector GGUF (`--mmproj`). `None` = text-only spawn — the
    /// caller resolves presence from the on-disk manifest and degrades to
    /// text-only (never a failed spawn) when the file is missing.
    pub mmproj: Option<PathBuf>,
}

/// Largest context window the device RAM can fund for a given model.
/// The budget takes the MIN of two views: 70% of total RAM (provisioning —
/// don't let llama eat the whole box) and available-at-spawn minus a margin
/// (reality — this process plus extensions may already consume most of RAM;
/// a total-only budget funded 3.4 GB of KV into a 3.0 GB-free NG4520 on
/// 2026-10-14 and the server OOM-died). A further 512 MB covers llama.cpp
/// compute/activation buffers. Returns min(requested, cap), floored at 8192
/// — the guard shrinks the window, it never disables the model.
///
/// Callers supply the per-model KV cost (bytes/token over full-attention
/// layers + context-independent state) from the registry; models without a
/// cost model skip the guard entirely.
pub fn cap_ctx_by_ram(
    requested: usize,
    total_ram_bytes: u64,
    available_ram_bytes: u64,
    size_bytes: u64,
    kv_bytes_per_token: u32,
    kv_const_bytes: u64,
) -> usize {
    const RUNTIME_HEADROOM: u64 = 512 * 1024 * 1024;
    // MemAvailable includes evictable file cache, so its margin is smaller
    // than RUNTIME_HEADROOM (which is subtracted from the budget below).
    const AVAILABLE_MARGIN: u64 = 256 * 1024 * 1024;
    let budget =
        (total_ram_bytes * 70 / 100).min(available_ram_bytes.saturating_sub(AVAILABLE_MARGIN));
    let avail = budget.saturating_sub(size_bytes + kv_const_bytes + RUNTIME_HEADROOM);
    let raw_cap = (avail / u64::from(kv_bytes_per_token)) as usize;
    // Floor to a power-of-two bucket (8K/16K/32K/64K/128K): a raw 81244 is
    // unexplainable in the UI/CLI, and every picker presents these buckets
    // anyway. Never rounds up — the guard shrinks, it never over-promises.
    let cap = floor_pow2_ctx(raw_cap.max(8192));
    requested.min(cap).max(8192)
}

/// Largest power of two ≤ n (n ≥ 1).
fn floor_pow2_ctx(n: usize) -> usize {
    if n.is_power_of_two() {
        n
    } else {
        n.next_power_of_two() >> 1
    }
}

#[derive(Clone)]
pub struct LlamaServerProcess {
    pub port: u16,
    child: std::sync::Arc<tokio::sync::Mutex<tokio::process::Child>>,
    /// Child pid, captured at spawn (tokio Child::id() after wait() returns
    /// None) — lets the RSS reader below work for the process lifetime.
    pid: Option<u32>,
}

/// Live llama-server handles spawned by this process (for graceful stop).
static LLAMA_SERVERS: std::sync::Mutex<Vec<LlamaServerProcess>> = std::sync::Mutex::new(Vec::new());

/// Stop every llama-server this process spawned. Called on graceful
/// shutdown; `kill_on_drop` covers the abnormal paths (crash, SIGKILL).
/// Idempotent.
pub fn stop_all_llama_servers() {
    let mut guard = LLAMA_SERVERS.lock().unwrap_or_else(|e| e.into_inner());
    let servers: Vec<LlamaServerProcess> = std::mem::take(&mut *guard);
    for s in servers {
        if let Ok(mut child) = s.child.try_lock() {
            // best-effort: kill_on_drop remains the backstop if locked
            let _ = child.start_kill();
        }
    }
}

/// Whether any spawned llama-server process is still alive. The watchdog
/// uses this to separate "the process is GONE (OOM-killed — respawn is
/// correct)" from "the process is alive but the health probe timed out
/// (busy loading / box under load — respawning would double-spawn two ~47 GB
/// address-space mappings and OOM the box, the 2026-10-15 near-miss). A
/// live-but-unresponsive server gets a kill-then-respawn escalation instead.
pub fn any_llama_server_alive() -> bool {
    let mut guard = LLAMA_SERVERS.lock().unwrap_or_else(|e| e.into_inner());
    guard.iter_mut().any(|s| s.is_alive())
}

/// Total resident memory of the LIVE llama-server(s). Exactly one server is
/// ever intended (the builtin spawn is single-instance), and every spawn
/// path REPLACES it — so a ctx budget computed "now" must add this back:
/// the old instance's RSS frees on replacement. Without this, a status poll
/// or respawn evaluated while the old server was alive saw an
/// available-starved budget and floored the window to 8192 (2026-10-15
/// incident: 3-second warn spam + a would-be respawn shrunk from 32768).
pub fn live_llama_rss_bytes() -> u64 {
    let guard = LLAMA_SERVERS.lock().unwrap_or_else(|e| e.into_inner());
    guard.iter().map(|s| s.rss_bytes()).sum()
}

fn page_size_bytes() -> u64 {
    static PAGE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *PAGE.get_or_init(|| {
        // AT_PAGESZ (auxv entry type 6) — page size without a libc dep;
        // entries are native-endian (u64 key, u64 value) pairs. Off-Linux
        // or unparseable, assume 4K (macOS/Windows are not llama-spawn
        // targets in practice, and the value only scales RSS estimates).
        std::fs::read("/proc/self/auxv")
            .ok()
            .and_then(|raw| {
                // auxv entries are native-endian (u64 key, u64 value)
                // pairs; step manually so a trailing partial entry can never
                // misparse as a full one.
                (0..raw.len().saturating_sub(15)).step_by(16).find_map(|i| {
                    let key = u64::from_ne_bytes(raw[i..i + 8].try_into().ok()?);
                    let val = u64::from_ne_bytes(raw[i + 8..i + 16].try_into().ok()?);
                    (key == 6 && val > 0).then_some(val)
                })
            })
            .unwrap_or(4096)
    })
}

pub async fn health_check(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/health", port);
    match reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

/// 循环探测直到健康或超时。供 wait_healthy 与测试共用。
pub async fn wait_healthy_loop(port: u16, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if health_check(port).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    false
}

/// Like [`wait_healthy_loop`], but ALSO returns early (false) when OUR child
/// exits before the port answers — the classic bind-failure case: a foreign
/// server already holds the port, the health probe succeeds against IT while
/// our llama-server died silently. Without the child check, `wait_healthy`
/// reported success against a server that was never ours (the port
/// misattribution the callers then guard with is_alive — a guard with a
/// startup race: a slow-starting child hasn't attempted the bind yet, so
/// try_wait() still says "running" and the misattribution slips through).
async fn wait_healthy_loop_checking_child(
    port: u16,
    timeout: Duration,
    child: &mut tokio::process::Child,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        // Child died before the port answered → bind failure (or crash):
        // fail immediately, never "succeed" against a foreign listener.
        if let Ok(Some(_)) = child.try_wait() {
            return false;
        }
        if health_check(port).await {
            // Settle window: a healthy answer may come from a FOREIGN
            // listener while our child is still starting and about to die
            // on bind (python3-style startup can take hundreds of ms). Stay
            // in the window long enough that a bind failure would surface
            // (~500 ms), re-checking both child liveness and the port.
            let settle = tokio::time::Instant::now() + Duration::from_millis(500);
            let mut died = false;
            while tokio::time::Instant::now() < settle {
                if let Ok(Some(_)) = child.try_wait() {
                    died = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if died {
                return false;
            }
            // Our child survived the settle window with the port healthy —
            // accept. (A legit llama-server never exits this fast.)
            return true;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    false
}

/// stderr sink for the spawned llama-server: a truncate-on-each-spawn
/// `server.log` beside the binary, falling back to null only if the file
/// can't be opened (read-only FS). One spawn's log — the current boot's
/// crash cause — is what matters; rotation is not worth the complexity.
fn server_stderr_stdio(binary: &std::path::Path) -> std::process::Stdio {
    let log_path = binary
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("server.log");
    match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
    {
        Ok(f) => f.into(),
        Err(e) => {
            tracing::warn!(
                log = %log_path.display(),
                error = %e,
                "llama-server stderr log unavailable — falling back to /dev/null"
            );
            std::process::Stdio::null()
        }
    }
}

/// Full llama-server argument vector for a config (everything except the
/// argv[0] binary). Extracted from `spawn` so the flag surface — including
/// the vision `--mmproj` — is unit-testable without constructing a child
/// process (the `LlamaServerProcess` fields are private by design).
///
/// Order matters only for the tests and human eyes; llama-server parses
/// flags positionally-independently.
fn build_llama_args(cfg: &LlamaServerConfig) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-m".into(),
        cfg.model.display().to_string(),
        "-c".into(),
        cfg.ctx.to_string(),
    ];
    if let Some(p) = &cfg.mmproj {
        // Vision projector: mounts the mtmd multimodal adapter. llama-server
        // then reports modalities.vision=true on /props, which the platform's
        // capability probe turns into supports_multimodal.
        args.push("--mmproj".into());
        args.push(p.display().to_string());
    }
    // Slot count, default 1. Why not leave it unspecified: upstream's
    // own default is 4 slots (verified on b11609: n_slots=4,
    // kv_unified=true) — and /props then reports total_slots>1, which
    // the platform budgets as n_ctx ÷ slots, silently shrinking the
    // usable window (the 8K-wedge class, observed 2026-10-14 on
    // NG4520). With one slot the full window is the per-request truth;
    // background LLM work already defers to foreground streams, so
    // concurrency here is rare — requests queue, which is the correct
    // behavior for a shared decode budget. NEOMIND_BUILTIN_LLM_PARALLEL:
    // N for explicit slots, 0 to omit the flag (upstream default).
    if let Some(p) = cfg.parallel {
        args.push("--parallel".into());
        args.push(p.to_string());
    }
    args.push("--port".into());
    args.push(cfg.port.to_string());
    args.push("--host".into());
    args.push("127.0.0.1".into());
    // `--no-webui`(非旧名 `--nobrowser`,当前 llama.cpp 已改名,旧参数报 invalid argument)
    args.push("--no-webui".into());
    if cfg.kv_quant {
        // -fa on is REQUIRED for V-cache quantization; q8_0 K+V roughly
        // halves the KV pool (see the config field's doc).
        args.extend(
            ["-fa", "on", "-ctk", "q8_0", "-ctv", "q8_0"]
                .into_iter()
                .map(String::from),
        );
    }
    if let Some(n) = cfg.ngl {
        args.push("-ngl".into());
        args.push(n.to_string());
    }
    if let Some(t) = cfg.threads {
        args.push("-t".into());
        args.push(t.to_string());
    }
    if let Some(t) = cfg.temperature {
        args.push("--temp".into());
        args.push(format!("{t:.2}"));
    }
    if let Some(p) = cfg.top_p {
        args.push("--top-p".into());
        args.push(format!("{p:.2}"));
    }
    if let Some(k) = cfg.top_k {
        args.push("--top-k".into());
        args.push(k.to_string());
    }
    if let Some(m) = cfg.min_p {
        args.push("--min-p".into());
        args.push(format!("{m:.2}"));
    }
    if let Some(r) = cfg.repeat_penalty {
        args.push("--repeat-penalty".into());
        args.push(format!("{r:.2}"));
    }
    args
}

impl LlamaServerProcess {
    pub fn spawn(cfg: &LlamaServerConfig) -> anyhow::Result<Self> {
        let mut cmd = tokio::process::Command::new(&cfg.binary);
        cmd.args(build_llama_args(cfg));
        cmd
            // stderr → truncate-on-spawn log next to the binary. /dev/null
            // made every startup death indistinguishable — the b11609 SIGILL
            // on NG4520 (2026-10-14) was only diagnosable by running the
            // binary by hand; the log makes the next one visible in place.
            .stderr(server_stderr_stdio(&cfg.binary))
            .stdout(std::process::Stdio::null());
        // [deterministic cleanup] The process must die with the server even
        // on abnormal termination (crash, kill -9, force-quit): without
        // kill_on_drop, a dropped handle leaked a model-loaded llama-server
        // (~2 GB) until reboot on EVERY restart path that didn't go through
        // an explicit stop(). Combined with the global registry below, the
        // handle is kept alive as long as the server runs, and kill_on_drop
        // makes process exit alone sufficient — SIGKILL included.
        cmd.kill_on_drop(true);
        let child = cmd.spawn()?;
        let pid = child.id();
        let proc = LlamaServerProcess {
            port: cfg.port,
            child: std::sync::Arc::new(tokio::sync::Mutex::new(child)),
            pid,
        };
        // Register globally so a graceful shutdown can stop it explicitly
        // (faster and cleaner than waiting for process-exit reap) — see
        // `stop_all_llama_servers`.
        LLAMA_SERVERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(proc.clone_handle());
        Ok(proc)
    }

    pub async fn wait_healthy(&mut self, timeout: Duration) -> anyhow::Result<()> {
        let healthy = {
            let mut child = match self.child.try_lock() {
                Ok(c) => c,
                Err(_) => anyhow::bail!("llama-server handle is being stopped concurrently"),
            };
            wait_healthy_loop_checking_child(self.port, timeout, &mut child).await
        };
        if healthy {
            return Ok(());
        }
        // Distinguish a dead child (crash / SIGILL / bind failure — the loop
        // returns within ~300ms of exit) from a genuine timeout. The old
        // message always blamed the timeout, which sent the b11609 SIGILL
        // investigation down the wrong path ("slow to load").
        let mut why = format!("did not become healthy in {:?}", timeout);
        if let Ok(mut child) = self.child.try_lock() {
            if let Ok(Some(status)) = child.try_wait() {
                why = format!(
                    "exited during startup: {} (see server.log beside the binary)",
                    status
                );
            }
        }
        anyhow::bail!("llama-server on :{} {}", self.port, why)
    }

    /// Whether the spawned child process is still alive.
    ///
    /// `wait_healthy` only probes the port — if another server already holds it,
    /// our child dies on bind while the probe succeeds against the foreign
    /// server. Callers must verify `is_alive()` after `wait_healthy` before
    /// trusting the child / registering an instance for it.
    pub fn is_alive(&mut self) -> bool {
        match self.child.try_lock() {
            Ok(mut child) => matches!(child.try_wait(), Ok(None)),
            Err(_) => true, // someone is stopping/waiting it — treat as alive
        }
    }

    /// Handle for the global registry (shares the same child).
    fn clone_handle(&self) -> LlamaServerProcess {
        LlamaServerProcess {
            port: self.port,
            child: self.child.clone(),
            pid: self.pid,
        }
    }

    /// Resident memory of this llama-server, from /proc (0 if gone or
    /// unreadable — callers treat that as "releases nothing").
    fn rss_bytes(&self) -> u64 {
        let Some(pid) = self.pid else { return 0 };
        let Ok(statm) = std::fs::read_to_string(format!("/proc/{pid}/statm")) else {
            return 0;
        };
        // statm: size resident shared text lib data dt — resident is field 2,
        // in pages.
        let resident_pages: u64 = statm
            .split_whitespace()
            .nth(1)
            .and_then(|f| f.parse().ok())
            .unwrap_or(0);
        resident_pages.saturating_mul(page_size_bytes())
    }

    pub async fn stop(self) -> anyhow::Result<()> {
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use std::time::Duration;

    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;

    fn test_server_config(mmproj: Option<PathBuf>) -> LlamaServerConfig {
        LlamaServerConfig {
            binary: PathBuf::from("/usr/bin/llama-server"),
            model: PathBuf::from("/data/models/gemma4-e2b/model.gguf"),
            port: 29375,
            ctx: 32768,
            ngl: None,
            threads: None,
            temperature: Some(1.0),
            top_p: Some(0.95),
            top_k: Some(64),
            min_p: None,
            repeat_penalty: None,
            kv_quant: true,
            parallel: Some(1),
            mmproj,
        }
    }

    /// The argv surface a spawn actually produces — locked so a refactor
    /// can't silently drop a flag (each one changes runtime behavior in
    /// ways the spawn sites' behavior tests would only catch downstream).
    #[test]
    fn build_args_without_mmproj_is_text_only() {
        let args = build_llama_args(&test_server_config(None));
        let joined = args.join(" ");
        assert!(joined.starts_with("-m /data/models/gemma4-e2b/model.gguf -c 32768"));
        assert!(joined.contains("--port 29375 --host 127.0.0.1 --no-webui"));
        assert!(joined.contains("--parallel 1"));
        assert!(joined.contains("-fa on -ctk q8_0 -ctv q8_0"));
        assert!(joined.contains("--temp 1.00 --top-p 0.95 --top-k 64"));
        assert!(
            !joined.contains("--mmproj"),
            "text-only spawn must not pass --mmproj"
        );
    }

    #[test]
    fn build_args_with_mmproj_places_flag_with_model() {
        let args = build_llama_args(&test_server_config(Some(PathBuf::from(
            "/data/models/gemma4-e2b/mmproj.gguf",
        ))));
        // Paired with -m/-c at the head — vision mounts with the model, and
        // a fixed position keeps the e2e fixture's marker parsing simple.
        assert_eq!(
            args[..6],
            [
                "-m",
                "/data/models/gemma4-e2b/model.gguf",
                "-c",
                "32768",
                "--mmproj",
                "/data/models/gemma4-e2b/mmproj.gguf"
            ]
            .map(String::from)
        );
    }

    #[test]
    fn eight_gb_jetson_caps_minicpm_below_default() {
        // MiniCPM5-2B on an Orin Nano 8GB: 5.5 GB of KV at 128K cannot fit.
        // Available ~= total on a freshly-booted box.
        let ctx = cap_ctx_by_ram(131_072, 8 * GB, 7 * GB, 1_561_318_368, 43_008, 0);
        assert!(ctx < 131_072, "must cap on 8GB, got {ctx}");
        assert!(ctx > 32_768, "cap should still buy headroom, got {ctx}");
    }

    #[test]
    fn replacement_adds_old_server_rss_back() {
        // The 2026-10-15 incident shape: 7.4GB box, 2.6GB free with the old
        // llama-server holding 1.2GB RSS. A REPLACEMENT spawn frees that RSS
        // (guarded_ctx adds live_llama_rss_bytes back), so the effective
        // available is 3.8GB — the window must not floor. Callers that
        // spawn WITHOUT replacing (none today — the builtin is
        // single-instance) would use the raw available figure.
        let without = cap_ctx_by_ram(32_768, 8 * GB, 2_600 * MB, 1_561_318_368, 43_008, 0);
        let with = cap_ctx_by_ram(
            32_768,
            8 * GB,
            2_600 * MB + 1_200 * MB,
            1_561_318_368,
            43_008,
            0,
        );
        assert!(
            with > without,
            "replacement must restore the window: {with} vs {without}"
        );
        assert_eq!(
            with, 32_768,
            "3.8GB effective available funds the full 32K window"
        );
    }

    #[test]
    fn page_size_is_sane() {
        let ps = page_size_bytes();
        assert!(
            ps.is_power_of_two(),
            "page size must be a power of two, got {ps}"
        );
        assert!(ps >= 4096);
    }

    #[test]
    fn live_rss_is_zero_with_no_servers() {
        // No llama-server registered in THIS test process — the reader must
        // degrade to 0, never panic.
        assert_eq!(live_llama_rss_bytes(), 0);
    }

    #[test]
    fn busy_box_caps_by_available_not_total() {
        // The NG4520 case (2026-10-14): 7.4 GB total but only ~3 GB free at
        // spawn — a total-only budget funded 3.4 GB of KV and OOM-killed the
        // server. The available view must clamp below the total view.
        let total_only = cap_ctx_by_ram(131_072, 8 * GB, 8 * GB, 1_561_318_368, 43_008, 0);
        let busy = cap_ctx_by_ram(131_072, 8 * GB, 3 * GB, 1_561_318_368, 43_008, 0);
        assert!(
            busy < total_only,
            "available must clamp: {busy} vs {total_only}"
        );
    }

    #[test]
    fn ram_rich_host_keeps_full_default() {
        // 32 GB host: budget 22.4 GB − weights − headroom ≫ 128K needs.
        let ctx = cap_ctx_by_ram(131_072, 32 * GB, 30 * GB, 1_561_318_368, 43_008, 0);
        assert_eq!(ctx, 131_072);
    }

    #[test]
    fn floor_is_8192_even_on_ridiculously_small_ram() {
        let ctx = cap_ctx_by_ram(131_072, 2 * GB, GB, 1_561_318_368, 43_008, 0);
        assert_eq!(ctx, 8192);
    }

    #[test]
    fn small_request_below_cap_untouched() {
        let ctx = cap_ctx_by_ram(8_192, 8 * GB, 7 * GB, 1_561_318_368, 43_008, 0);
        assert_eq!(ctx, 8_192);
    }

    #[test]
    fn qwen_linear_state_const_counts_against_budget() {
        // The context-independent state must reduce the cap. Pow2 bucketing
        // collapses sub-bucket differences (Qwen's real ~201 MB const lands
        // both caps in the 64K bucket), so use an exaggerated const to keep
        // the reduction crossing a bucket boundary and observable.
        let with_const = cap_ctx_by_ram(
            131_072,
            8 * GB,
            7 * GB,
            2_740_000_000,
            32_768,
            1_500_000_000,
        );
        let without = cap_ctx_by_ram(131_072, 8 * GB, 7 * GB, 2_740_000_000, 32_768, 0);
        assert!(with_const < without, "{with_const} vs {without}");
    }

    #[test]
    fn capped_results_are_power_of_two_buckets() {
        // 8GB/fresh: raw cap ~82-84K — must surface as 65536, not 81244.
        let ctx = cap_ctx_by_ram(131_072, 8 * GB, 7 * GB, 1_561_318_368, 43_008, 0);
        assert!(ctx.is_power_of_two(), "bucketed cap expected, got {ctx}");
        assert_eq!(ctx, 65_536);
    }

    #[test]
    fn floor_pow2_rounds_down_only() {
        assert_eq!(floor_pow2_ctx(8_192), 8_192);
        assert_eq!(floor_pow2_ctx(8_193), 8_192);
        assert_eq!(floor_pow2_ctx(81_244), 65_536);
        assert_eq!(floor_pow2_ctx(131_072), 131_072);
    }

    #[tokio::test]
    async fn health_check_true_when_server_up() {
        let router = Router::new().route("/health", get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let h = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        // 稍等 server 就绪
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(health_check(addr.port()).await);
        h.abort();
    }

    #[tokio::test]
    async fn health_check_false_when_nothing_listening() {
        // 挑一个几乎肯定没人监听的端口:绑定后立刻释放再测,极小概率冲突
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        assert!(!health_check(port).await);
    }

    #[tokio::test]
    async fn wait_healthy_polls_until_ready() {
        let router = Router::new().route("/health", get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let h = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        // 不构造 LlamaServerProcess(其 child 字段私有、无法手工构造),
        // 直接测公开的 wait_healthy_loop(port, timeout):wait_healthy 只依赖 port。
        assert!(wait_healthy_loop(addr.port(), Duration::from_secs(3)).await);
        h.abort();
    }
}
