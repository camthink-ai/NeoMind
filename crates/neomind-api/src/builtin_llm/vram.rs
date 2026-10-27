//! GPU offload sizing for the builtin llama-server (discrete NVIDIA GPUs).
//!
//! The spawn paths historically left `-ngl` to llama-server's own default —
//! full offload — which is right on an empty GPU and a CUDA-OOM death on a
//! shared one (the 2026-10-27 T4 incident: five GPU processes had eaten
//! 15.7 of 16 GiB, so an operator hand-pinned `NEOMIND_BUILTIN_MODEL_NGL=8`
//! in a systemd drop-in to keep the model alive — and the pin outlived the
//! crisis, capping a 2B model at 1/4 of the achievable speed with no code
//! able to notice).
//!
//! This module sizes the offload at spawn time instead: probe free VRAM via
//! `nvidia-smi`, charge the model's weights + projector + (proportional) KV
//! cache + a fixed CUDA-context/compute reserve against it, and pick a layer
//! count that fits. Every failure mode — no `nvidia-smi` (CPU boxes, Jetson
//! unified memory), a wedged driver, an unparseable GGUF header — falls back
//! to the previous behavior exactly. An explicit operator pin
//! (`NEOMIND_BUILTIN_MODEL_NGL` / `NEOMIND_BUILTIN_VLM_NGL`) ALWAYS wins and
//! is never second-guessed.

use std::path::Path;

use super::gguf::parse_gguf;

const MIB: u64 = 1024 * 1024;

/// Fixed charge against the VRAM budget before any layers: the CUDA context,
/// compute/activation buffers, and a margin for external processes growing
/// between the probe and the allocation. Observed floor on a T4
/// (2026-10-27, MiniCPM5-2B @ -ngl 8, ctx 8192): ~476 MiB beyond layer
/// weights + KV.
const FIXED_RESERVE_BYTES: u64 = 512 * MIB;

/// KV charge when the model has no registry cost model (companion VLMs,
/// custom imports): a 1B-scale VLM at its small default window.
/// Over-charging beats a CUDA OOM at spawn.
const UNKNOWN_KV_FLAT_BYTES: u64 = 512 * MIB;

/// Haircut on (free + reclaim): allocator fragmentation plus external
/// processes growing between the probe and llama-server's allocation.
const BUDGET_HAIRCUT_PERCENT: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuMemory {
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// Probe total/free VRAM across all GPUs via `nvidia-smi`. `None` on any
/// failure (binary absent, non-zero exit, unparseable output, or a hang —
/// the same 5s-watchdog treatment as the runtime's `NvidiaGpuProbe`: a
/// broken driver can wedge nvidia-smi and auto-sizing must not be able to
/// break boot). `None` on Jetson too (no nvidia-smi on Tegra — unified
/// memory is already budgeted by the RAM guard).
pub fn probe_gpu_memory() -> Option<GpuMemory> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = std::process::Command::new("nvidia-smi")
            .args([
                "--query-gpu=memory.total,memory.free",
                "--format=csv,noheader,nounits",
            ])
            .output();
        let _ = tx.send(out);
    });
    let out = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .ok()?
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_gpu_memory_csv(&String::from_utf8_lossy(&out.stdout))
}

/// "24576, 15876" per GPU row (MiB, noheader+nounits) → summed bytes.
/// `None` when any row is unparseable — a wedged driver's garbage must not
/// pass as a (wrong) budget. An empty output is also None: zero GPUs probed
/// means we know nothing.
fn parse_gpu_memory_csv(s: &str) -> Option<GpuMemory> {
    let mut total = 0u64;
    let mut free = 0u64;
    let mut rows = 0u64;
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split(',');
        let t: u64 = parts.next()?.trim().parse().ok()?;
        let f: u64 = parts.next()?.trim().parse().ok()?;
        total += t;
        free += f;
        rows += 1;
    }
    (rows > 0).then_some(GpuMemory {
        total_bytes: total * MIB,
        free_bytes: free * MIB,
    })
}

/// VRAM currently held by the given llama-server pids, from
/// `nvidia-smi --query-compute-apps`. Used for replacement semantics: the
/// server being replaced frees its VRAM, so its usage counts back into the
/// budget. 0 on any failure — undercounting reclaim only shrinks the
/// budget, which is the safe direction.
pub fn llama_vram_bytes(pids: &[u32]) -> u64 {
    if pids.is_empty() {
        return 0;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = std::process::Command::new("nvidia-smi")
            .args([
                "--query-compute-apps=pid,used_memory",
                "--format=csv,noheader,nounits",
            ])
            .output();
        let _ = tx.send(out);
    });
    let Ok(Ok(out)) = rx.recv_timeout(std::time::Duration::from_secs(5)) else {
        return 0;
    };
    if !out.status.success() {
        return 0;
    }
    parse_compute_apps_csv(&String::from_utf8_lossy(&out.stdout), pids)
}

/// "870990, 798" rows (pid, MiB), summed over the wanted pids. Rows that
/// don't parse (some drivers print "[Not Supported]" for used_memory) are
/// skipped, not fatal.
fn parse_compute_apps_csv(s: &str, pids: &[u32]) -> u64 {
    let mut sum = 0u64;
    for line in s.lines() {
        let mut parts = line.split(',');
        let (Some(pid_s), Some(mem_s)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Ok(pid) = pid_s.trim().parse::<u32>() else {
            continue;
        };
        let Ok(mem_mib) = mem_s.trim().parse::<u64>() else {
            continue;
        };
        if pids.contains(&pid) {
            sum += mem_mib * MIB;
        }
    }
    sum
}

/// Whether the llama-server binary can act on `-ngl` against an NVIDIA GPU:
/// our downloaded CUDA build (cache dir `…-cuda`), or a bundled binary
/// (variant unknown — the probe itself decides whether a GPU exists).
/// CPU/Vulkan/Jetson caches and anything unrecognized: skip auto-sizing.
/// Jetson is the important exclusion — unified memory means the RAM guard
/// already budgets the same physical bytes, and forcing `-ngl` there would
/// double-count them.
pub fn binary_gpu_capable(binary: &Path) -> bool {
    let name = binary.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if name.starts_with("neomind-llama-server") {
        return true;
    }
    binary
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .is_some_and(|d| d.ends_with("-cuda"))
}

/// Everything the layer-budget solver needs, gathered by the caller so the
/// solver itself is pure and unit-testable without a GPU.
#[derive(Debug, Clone)]
pub struct GpuBudgetInputs {
    /// Free VRAM at probe time (excludes everything currently allocated,
    /// including our own live llama-servers).
    pub free_bytes: u64,
    /// VRAM our llama-server being REPLACED still holds — it frees on the
    /// replacement spawn, so it counts back into the budget (the VRAM
    /// analogue of the RAM guard's live_llama_rss add-back).
    pub reclaim_bytes: u64,
    /// Model file size on disk (weights; over-estimates the per-layer GPU
    /// share because embeddings only offload on a full offload — the
    /// conservative direction).
    pub weights_bytes: u64,
    /// Vision projector file size (offloads wholly when attached).
    pub mmproj_bytes: u64,
    /// Total decoder layers, from the GGUF header (`<arch>.block_count`).
    pub total_layers: u32,
    /// Context window the spawn will use.
    pub ctx: usize,
    /// Registry KV cost model (fp16 K+V bytes/token over full-attention
    /// layers). `None` = no cost model → flat [`UNKNOWN_KV_FLAT_BYTES`]
    /// charge instead.
    pub kv_bytes_per_token_f16: Option<u32>,
    /// Context-independent KV state (sliding-window/linear layers), charged
    /// in full (conservative: only the offloaded fraction really moves).
    pub kv_const_bytes: Option<u64>,
    /// q8_0 KV quantization active (the spawn default) — halves the
    /// per-token cost, mirroring the RAM guard's convention.
    pub kv_quant: bool,
}

/// The auto offload decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffloadPlan {
    pub ngl: u16,
    pub total_layers: u32,
    /// Human-readable basis for the spawn log line.
    pub note: String,
}

/// Solve how many layers fit the VRAM budget. Full weights + full KV fit →
/// full offload. Otherwise layers are solved proportionally: a layer's KV
/// lives where the layer lives, so each offloaded layer costs its weights
/// plus its 1/N share of the KV bill. Zero affordable → `-ngl 0` (explicit
/// CPU spawn — on a CUDA build that keeps the KV cache off the GPU too, so
/// a saturated GPU degrades to slow-but-alive instead of a spawn that dies
/// on its first cudaMalloc).
pub fn plan_gpu_layers(i: &GpuBudgetInputs) -> OffloadPlan {
    let budget = (i.free_bytes + i.reclaim_bytes) * (100 - BUDGET_HAIRCUT_PERCENT) / 100;
    // Registry KV is the f16 figure; q8_0 (spawn default) ≈ half — model
    // what is actually allocated, same convention as guarded_ctx.
    let kv_tok = i.kv_bytes_per_token_f16.map(|k| {
        let k = u64::from(k);
        if i.kv_quant { k / 2 } else { k }
    });
    let kv_full = match kv_tok {
        Some(t) => t.saturating_mul(i.ctx as u64) + i.kv_const_bytes.unwrap_or(0),
        None => UNKNOWN_KV_FLAT_BYTES + i.kv_const_bytes.unwrap_or(0),
    };
    let fixed = FIXED_RESERVE_BYTES + i.mmproj_bytes;
    let full_cost = i.weights_bytes + kv_full + fixed;
    if full_cost <= budget {
        return OffloadPlan {
            ngl: u16::try_from(i.total_layers).unwrap_or(u16::MAX),
            total_layers: i.total_layers,
            note: format!(
                "full offload: {} MiB cost ≤ {} MiB budget",
                full_cost / MIB,
                budget / MIB
            ),
        };
    }
    let per_layer = i.weights_bytes / u64::from(i.total_layers.max(1));
    let kv_per_layer = kv_full / u64::from(i.total_layers.max(1));
    let denom = per_layer + kv_per_layer;
    let affordable = budget.saturating_sub(fixed);
    // denom 0 (zero-size model edge) → full offload rather than a div-by-zero.
    let layers = (affordable / denom.max(1)).min(u64::from(i.total_layers));
    OffloadPlan {
        ngl: layers as u16,
        total_layers: i.total_layers,
        note: format!(
            "partial: full cost {} MiB > budget {} MiB (free {} + reclaim {} − {}% haircut)",
            full_cost / MIB,
            budget / MIB,
            i.free_bytes / MIB,
            i.reclaim_bytes / MIB,
            BUDGET_HAIRCUT_PERCENT
        ),
    }
}

/// The last spawn's offload decision, recorded per port for the status API.
/// The 2026-10-27 T4 incident was diagnosable only from inside a shell
/// session (`ps` for the flag, `nvidia-smi` for the why); this record makes
/// the next one visible from the API/UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LastNglDecision {
    /// Effective `-ngl` of the last spawn. `null` = no flag passed
    /// (llama-server default — full offload — or auto-sizing skipped).
    pub ngl: Option<u16>,
    pub total_layers: Option<u32>,
    /// Free VRAM at decision time (MiB); `null` when no probe ran or it
    /// failed.
    pub free_mib: Option<u64>,
    /// VRAM the replaced same-port server freed (MiB).
    pub reclaim_mib: u64,
    pub ctx: usize,
    /// Why: "pinned by config" | the plan's arithmetic | the skip reason.
    pub note: String,
}

/// Decisions keyed by llama-server port (main 29375 / companion 29385 in
/// the default layout): the companion spawns after the main model, so a
/// single slot would always show the companion's decision under the main
/// status block. A Vec — two entries max in practice, and HashMap::new is
/// not const-initializable in a static.
static DECISIONS: std::sync::Mutex<Vec<(u16, LastNglDecision)>> = std::sync::Mutex::new(Vec::new());

fn record_decision(port: u16, d: LastNglDecision) {
    let mut guard = DECISIONS.lock().unwrap_or_else(|e| e.into_inner());
    guard.retain(|(p, _)| *p != port);
    guard.push((port, d));
}

/// The most recent offload decision for the llama-server on `port`
/// (`None` = no spawn since process start).
pub fn last_decision_on(port: u16) -> Option<LastNglDecision> {
    DECISIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(p, _)| *p == port)
        .map(|(_, d)| d.clone())
}

/// The spawn-site query: an explicit pin wins; otherwise auto-size when the
/// binary can honor it, a discrete NVIDIA GPU answers the probe, and the
/// GGUF header reports its layer count. Any miss → `None` = pass no flag =
/// llama-server's own default, the exact pre-auto behavior. Every path
/// records its decision for the status API.
pub struct NglQuery<'a> {
    /// `NEOMIND_BUILTIN_MODEL_NGL` / `NEOMIND_BUILTIN_VLM_NGL` (after the
    /// companion-override merge in `spawn_companion_server`).
    pub explicit: Option<u16>,
    pub binary: &'a Path,
    pub model_path: &'a Path,
    pub mmproj: Option<&'a Path>,
    pub ctx: usize,
    /// Port of the llama-server being spawned — scopes the replacement
    /// reclaim to the process being replaced. Servers on OTHER ports (the
    /// vision companion during a main-model respawn) stay running and their
    /// VRAM must not inflate this budget.
    pub port: u16,
    pub kv_bytes_per_token_f16: Option<u32>,
    pub kv_const_bytes: Option<u64>,
    pub kv_quant: bool,
}

pub fn resolve_spawn_ngl(q: &NglQuery<'_>) -> Option<u16> {
    if let Some(n) = q.explicit {
        tracing::debug!(ngl = n, "builtin llm: ngl pinned by config — auto-sizing skipped");
        record_decision(
            q.port,
            LastNglDecision {
                ngl: Some(n),
                total_layers: parse_gguf(q.model_path).ok().and_then(|m| m.block_count),
                // Best-effort observability only — a probe failure must not
                // second-guess the operator's pin.
                free_mib: probe_gpu_memory().map(|m| m.free_bytes / MIB),
                reclaim_mib: 0,
                ctx: q.ctx,
                note: "pinned by config (NEOMIND_BUILTIN_MODEL_NGL / _VLM_NGL)".into(),
            },
        );
        return Some(n);
    }
    let skip = |note: String| {
        record_decision(
            q.port,
            LastNglDecision {
                ngl: None,
                total_layers: None,
                free_mib: None,
                reclaim_mib: 0,
                ctx: q.ctx,
                note,
            },
        );
    };
    if !binary_gpu_capable(q.binary) {
        skip("cpu/vulkan/jetson build — no GPU offload sizing (llama-server default)".into());
        return None;
    }
    let Some(mem) = probe_gpu_memory() else {
        skip("no nvidia-smi / probe failed — llama-server default (full offload)".into());
        return None; // CPU box, Jetson, or a wedged driver
    };
    let Some(total_layers) = parse_gguf(q.model_path)
        .ok()
        .and_then(|m| m.block_count)
        .filter(|l| *l > 0)
    else {
        skip("GGUF header lacks block_count — auto sizing skipped (llama-server default)".into());
        return None;
    };
    let weights = std::fs::metadata(q.model_path).map(|m| m.len()).ok()?;
    if weights == 0 {
        skip("model file empty — auto sizing skipped".into());
        return None;
    }
    let mmproj_bytes = q
        .mmproj
        .and_then(|p| std::fs::metadata(p).map(|m| m.len()).ok())
        .unwrap_or(0);
    let reclaim = llama_vram_bytes(&super::server::live_llama_pids_on_port(q.port));
    let plan = plan_gpu_layers(&GpuBudgetInputs {
        free_bytes: mem.free_bytes,
        reclaim_bytes: reclaim,
        weights_bytes: weights,
        mmproj_bytes,
        total_layers,
        ctx: q.ctx,
        kv_bytes_per_token_f16: q.kv_bytes_per_token_f16,
        kv_const_bytes: q.kv_const_bytes,
        kv_quant: q.kv_quant,
    });
    tracing::info!(
        ngl = plan.ngl,
        layers = plan.total_layers,
        free_mib = mem.free_bytes / MIB,
        reclaim_mib = reclaim / MIB,
        ctx = q.ctx,
        "builtin llm: auto GPU offload sized ({})",
        plan.note
    );
    record_decision(
        q.port,
        LastNglDecision {
            ngl: Some(plan.ngl),
            total_layers: Some(plan.total_layers),
            free_mib: Some(mem.free_bytes / MIB),
            reclaim_mib: reclaim / MIB,
            ctx: q.ctx,
            note: plan.note,
        },
    );
    Some(plan.ngl)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * MIB;

    fn inputs(free: u64) -> GpuBudgetInputs {
        // MiniCPM5-2B shape (2026-10-27 T4): ~1.5 GiB q4_k_m, 34 layers,
        // 32K ctx, 43008 B/token f16 KV, q8 active.
        GpuBudgetInputs {
            free_bytes: free,
            reclaim_bytes: 0,
            weights_bytes: 1_561_318_368,
            mmproj_bytes: 0,
            total_layers: 34,
            ctx: 32_768,
            kv_bytes_per_token_f16: Some(43_008),
            kv_const_bytes: Some(0),
            kv_quant: true,
        }
    }

    #[test]
    fn full_offload_when_budget_covers() {
        let p = plan_gpu_layers(&inputs(8 * GIB));
        assert_eq!(p.ngl, 34, "empty 8 GiB GPU must fully offload: {}", p.note);
    }

    #[test]
    fn saturated_gpu_degrades_to_cpu_only() {
        // The incident box: 622 MiB free. Budget 559 MiB − 512 reserve
        // leaves ~47 MiB — not one layer. ngl 0 = an explicit CPU spawn
        // that stays alive instead of dying on its first cudaMalloc.
        let p = plan_gpu_layers(&inputs(622 * MIB));
        assert_eq!(p.ngl, 0, "saturated GPU must not claim layers: {}", p.note);
    }

    #[test]
    fn moderately_full_gpu_gets_partial_layers() {
        // ~2.5 GiB free: full cost (~2.7 GiB) doesn't fit, but most layers
        // do — partial offload, not all-or-nothing.
        let p = plan_gpu_layers(&inputs(2_560 * MIB));
        assert!(p.ngl > 0 && p.ngl < 34, "expected partial, got {}: {}", p.ngl, p.note);
    }

    #[test]
    fn haircut_applies_so_exact_fit_is_not_full() {
        // free == full_cost exactly: the 10% haircut must push it to
        // partial — the budget may not promise bytes the GPU doesn't have.
        let mut i = inputs(0);
        i.weights_bytes = 1_000 * MIB;
        i.kv_bytes_per_token_f16 = Some(0);
        i.total_layers = 10;
        let full_cost = 1_000 * MIB + FIXED_RESERVE_BYTES;
        i.free_bytes = full_cost;
        let p = plan_gpu_layers(&i);
        assert!(p.ngl < 10, "haircut must deny the exact fit: {}", p.note);
    }

    #[test]
    fn replacement_reclaim_counts_toward_budget() {
        let without = plan_gpu_layers(&inputs(622 * MIB));
        let mut i = inputs(0);
        i.reclaim_bytes = 8 * GIB; // old server on OUR port frees 8 GiB
        let with = plan_gpu_layers(&i);
        assert_eq!(without.ngl, 0);
        assert_eq!(with.ngl, 34, "reclaim must restore the full offload");
    }

    #[test]
    fn unknown_kv_model_is_charged_a_flat_reserve() {
        // No cost model (companion VLM / custom import): the flat charge
        // must reduce the affordable layers vs a known-zero KV cost.
        let mut known = inputs(2 * GIB);
        known.kv_bytes_per_token_f16 = Some(0);
        let mut unknown = known.clone();
        unknown.kv_bytes_per_token_f16 = None;
        assert!(plan_gpu_layers(&unknown).ngl <= plan_gpu_layers(&known).ngl);
    }

    #[test]
    fn gpu_memory_csv_parsing() {
        let one = parse_gpu_memory_csv("24576, 15876").expect("single row");
        assert_eq!(one.total_bytes, 24_576 * MIB);
        assert_eq!(one.free_bytes, 15_876 * MIB);
        let two = parse_gpu_memory_csv("16384, 0\n24576, 15876\n").expect("two rows");
        assert_eq!(two.free_bytes, 15_876 * MIB);
        assert!(parse_gpu_memory_csv("").is_none(), "no GPUs probed");
        assert!(
            parse_gpu_memory_csv("24576, [N/A]").is_none(),
            "garbage must not half-trust as a budget"
        );
    }

    #[test]
    fn compute_apps_csv_filters_pids_and_skips_unparseable() {
        let csv = "326984, 5912\n870990, 798\n1076618, [Not Supported]\n";
        assert_eq!(parse_compute_apps_csv(csv, &[870_990]), 798 * MIB);
        assert_eq!(parse_compute_apps_csv(csv, &[999]), 0, "foreign pids ignored");
        // The unparseable row belongs to a foreign pid anyway; ours parses.
        assert_eq!(parse_compute_apps_csv("870990, 798", &[870_990]), 798 * MIB);
    }

    #[test]
    fn decisions_are_recorded_per_port() {
        // The companion spawns after the main model — a single global slot
        // would always show the companion's decision under the main status.
        record_decision(
            29375,
            LastNglDecision {
                ngl: Some(34),
                total_layers: Some(34),
                free_mib: Some(8000),
                reclaim_mib: 0,
                ctx: 32768,
                note: "test-main".into(),
            },
        );
        record_decision(
            29385,
            LastNglDecision {
                ngl: None,
                total_layers: None,
                free_mib: None,
                reclaim_mib: 0,
                ctx: 8192,
                note: "test-companion".into(),
            },
        );
        assert_eq!(last_decision_on(29375).unwrap().note, "test-main");
        assert_eq!(last_decision_on(29375).unwrap().ngl, Some(34));
        assert_eq!(last_decision_on(29385).unwrap().note, "test-companion");
        assert!(last_decision_on(29999).is_none(), "unspawned port has no record");
    }

    #[test]
    fn binary_capability_by_cache_layout() {
        let cuda = Path::new("/var/lib/neomind/data/llama-server/b11609-cuda/llama-server");
        assert!(binary_gpu_capable(cuda));
        let cpu = Path::new("/var/lib/neomind/data/llama-server/b11609/llama-server");
        assert!(!binary_gpu_capable(cpu), "plain version dir = CPU cache");
        let vulkan = Path::new("/x/b11609-vulkan/llama-server");
        assert!(!binary_gpu_capable(vulkan));
        let jetson = Path::new("/x/b11609-jetson/llama-server");
        assert!(!binary_gpu_capable(jetson), "unified memory — RAM guard owns it");
        let bundled = Path::new("/usr/local/bin/neomind-llama-server");
        assert!(binary_gpu_capable(bundled), "variant unknown — probe decides");
    }
}
