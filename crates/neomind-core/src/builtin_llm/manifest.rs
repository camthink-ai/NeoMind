use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Default builtin model. `BUILTIN_MODELS` (below) is the full registry; the
/// user picks which one to install, this is the default/recommended entry.
pub const BUILTIN_MODEL_ID: &str = "minicpm5-2b";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelManifest {
    pub id: String,
    pub version: String,
    pub file_name: String,
    pub sha256: String,
    pub quant: String,
}

impl ModelManifest {
    pub fn model_path(&self, models_dir: &Path) -> PathBuf {
        models_dir.join(&self.id).join(&self.file_name)
    }
}

/// One installable builtin model (registry entry).
#[derive(Clone)]
pub struct BuiltinModelDef {
    /// Canonical local file name + sha (stored under `models/<id>/`).
    pub manifest: ModelManifest,
    /// Human name shown in the picker (e.g. "Qwen3.5-4B").
    pub display_name: &'static str,
    /// HuggingFace repo hosting `hf_file`.
    pub hf_repo: &'static str,
    /// Filename *in the HF repo* (may differ from the canonical `file_name`).
    pub hf_file: &'static str,
    /// Download size in bytes (for the picker).
    pub size_bytes: u64,
    /// Default context window for this model. Defaults are generous (128K
    /// where the arch allows) — the spawn path caps the window to what the
    /// device RAM can fund (see `kv_bytes_per_token` + cap_ctx_by_ram).
    pub default_ctx: u32,
    /// Native context ceiling (display); default_ctx is the run default.
    pub max_ctx: u32,
    /// KV-cache cost model backing the spawn-time RAM guard, exact from the
    /// HF config (fp16 K+V): bytes/token over the FULL-attention layers,
    /// plus context-independent state (sliding-window layers keep a fixed
    /// window; linear-attention layers keep a fixed recurrence). None = no
    /// guard data → the RAM cap is skipped for this model.
    pub kv_bytes_per_token: Option<u32>,
    pub kv_const_bytes: Option<u64>,
    /// Per-model sampling defaults (None = platform-wide legacy default).
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    /// min_p override (llama.cpp default 0.05). MiniCPM5's official card
    /// requires 0.0 — see the model entry.
    pub min_p: Option<f32>,
    /// repeat_penalty override (llama.cpp default 1.0 = off). Part of the
    /// official cards that specify it (MiniCPM 1.05, LFM 1.1).
    pub repeat_penalty: Option<f32>,
    /// One-line capability note for the picker.
    pub notes: &'static str,
    /// This entry is a fallback if the model cannot run (reserved).
    pub recommended: bool,
    /// Whether thinking/reasoning is enabled by default for this model.
    /// LFM's thinking is integral (cannot be off); Qwen3.5 runs faster
    /// non-thinking for agent tool calls (eval 76% cmd_ok was non-thinking);
    /// Gemma defaults to thinking.
    pub default_thinking: bool,

    /// Thinking cannot be turned off for this model (LFM's template ignores
    /// the toggle). A MODEL property — must never be derived from "is this
    /// the default" (that comparison silently flipped when the default
    /// moved to MiniCPM and mis-flagged LFM installs on the restart path).
    pub thinking_is_integral: bool,
    /// Recommended MINIMUM available RAM (MB): weights + KV cache + runtime
    /// + OS headroom. The UI discourages (not blocks) installs below this.
    pub min_ram_mb: u64,
}

/// The builtin model registry. MiniCPM5-2B is the default (2026-09
/// corrected-harness eval: 81% tool accuracy @8K, most robust across
/// context windows, Apache-2.0); Qwen3.5-4B is the strongest agent when
/// given 16K; Gemma4-E2B adds a QAT + vision-friendly option; LFM2.5 keeps
/// the native-128K niche.
pub static BUILTIN_MODELS: LazyLock<Vec<BuiltinModelDef>> = LazyLock::new(|| {
    vec![
        BuiltinModelDef {
            manifest: ModelManifest {
                id: "minicpm5-2b".to_string(),
                version: "1.0".to_string(),
                file_name: "minicpm5-2b-q4_k_m.gguf".to_string(),
                // Official openbmb LFS oid (captured 2026-09-14 via the
                // repo's LFS pointer). NOTE: the previous pin pointed at a
                // THIRD-PARTY mirror (Abiray) whose bytes differ (2080-byte
                // size delta — a repack); switching sources means switching
                // hashes. Already-downloaded installs keep their file: the
                // sha only gates NEW downloads.
                sha256: "ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd"
                    .to_string(),
                quant: "q4_k_m".to_string(),
            },
            display_name: "MiniCPM5-2B",
            // Official openbmb card (2026-10 official-first policy):
            // temperature=1.0, top_p=0.95, min_p=0.0, repetition_penalty
            // 1.05. The card + OpenBMB issue #374 warn llama.cpp's default
            // min_p=0.05 causes repetition loops on this model — min_p=0
            // and rep 1.05 are REQUIRED parts of the official point, not
            // optional extras. No top_k on the card; 40 = llama.cpp
            // default. The prior focused sampler (0.6/0.85/40, no
            // min_p/rep overrides) is what the 2026-09 eval's 81% tool
            // accuracy was measured at.
            temperature: Some(1.0),
            top_p: Some(0.95),
            top_k: Some(40),
            min_p: Some(0.0),
            repeat_penalty: Some(1.05),
            // Official model-author repo — the previous third-party mirror
            // (Abiray) served a byte-different repack and personal repos
            // vanish; openbmb is the durable, authoritative source.
            hf_repo: "openbmb/MiniCPM5-2B-GGUF",
            hf_file: "MiniCPM5-2B-Q4_K_M.gguf",
            size_bytes: 1_561_320_448,
            // 128K default (2026-10-10, user demand: real agent sessions
            // exceed 32K): dense arch so KV is the expensive kind — 42L ×
            // 2kv × 128hd × 2(K+V) × 2B = 43,008 B/token → 5.5 GB at 128K.
            // Fine on RAM-rich hosts; the spawn RAM guard caps small
            // devices (~91K on an 8 GB Orin). Eval profile is flat across
            // windows and the native ceiling is 128K.
            default_ctx: 131072,
            max_ctx: 131072,
            kv_bytes_per_token: Some(43_008),
            kv_const_bytes: Some(0),
            notes: "2026-09 评测首选 — 工具命中 81%、全窗口最稳、Apache-2.0 可分发",
            recommended: true,
            default_thinking: true,
            thinking_is_integral: false,
            min_ram_mb: 3_072,
        },
        BuiltinModelDef {
            manifest: ModelManifest {
                id: "lfm25-2.6b".to_string(),
                version: "1.0".to_string(),
                file_name: "lfm25-2.6b-qad_q4_0.gguf".to_string(),
                sha256: "a247afd6414918eac8e520a9e6137dc271235461ecbe1180462221d5b8d40b03"
                    .to_string(),
                quant: "qad_q4_0".to_string(),
            },
            display_name: "LFM2.5-2.6B",
            // Official card (2026-10 official-first policy):
            // generation_config temperature=0.1, top_k=50,
            // repetition_penalty=1.1 (top_p unset → 1.0 = off). The
            // superseded measured-best 0.6/0.85/20 scored 58% vs official
            // 55% on the 154-case A/B — inside the ~7pp noise floor.
            temperature: Some(0.1),
            top_p: Some(1.0),
            top_k: Some(50),
            min_p: None,
            repeat_penalty: Some(1.1),
            hf_repo: "LiquidAI/LFM2.5-2.6B-GGUF",
            hf_file: "LFM2.5-2.6B-QAD-Q4_0.gguf",
            size_bytes: 1_500_000_000,
            default_ctx: 131072,
            max_ctx: 131072,
            // Hybrid: 10 full-attn L × 8kv × 64hd × 2 × 2B (head_dim not in
            // the config; 64 is the LFM2 family value) → 20,480 B/token;
            // 20 conv layers hold fixed recurrence state (~1 MB, ignored).
            kv_bytes_per_token: Some(20_480),
            kv_const_bytes: Some(0),
            notes: "原生 128K 上下文(hybrid KV 很省)— 长会话细分场景;默认首选已让位 MiniCPM5-2B",
            recommended: false,
            default_thinking: true,
            thinking_is_integral: true,
            min_ram_mb: 3_072,
        },
        BuiltinModelDef {
            manifest: ModelManifest {
                id: "qwen3.5-4b".to_string(),
                version: "1.0".to_string(),
                file_name: "qwen3.5-4b-q4_k_m.gguf".to_string(),
                sha256: "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4"
                    .to_string(),
                quant: "q4_k_m".to_string(),
            },
            display_name: "Qwen3.5-4B",
            // Official non-thinking recommendation (qwen.readthedocs.io).
            temperature: Some(0.7),
            top_p: Some(0.8),
            top_k: Some(20),
            min_p: None,
            repeat_penalty: None,
            hf_repo: "unsloth/Qwen3.5-4B-GGUF",
            hf_file: "Qwen3.5-4B-Q4_K_M.gguf",
            size_bytes: 2_740_000_000,
            // 128K default (2026-10-10): hybrid arch — 24 of 32 layers are
            // linear attention (fixed recurrence, f32 state 24L × 32v ×
            // 128×128 × 4B ≈ 201 MB const), 8 full-attn L × 4kv × 256hd ×
            // 2 × 2B = 32,768 B/token → 4.2 GB at 128K. RAM-rich hosts run
            // the full window; the spawn guard caps small devices.
            default_ctx: 131072,
            max_ctx: 262144,
            kv_bytes_per_token: Some(32_768),
            kv_const_bytes: Some(201_326_592),
            notes:
                "端侧最强 agent 之一(2026-09 评测 70 分@16K)— 8K 下会大幅退化;可挂 mmproj 加视觉",
            recommended: false,
            default_thinking: false,
            thinking_is_integral: false,
            min_ram_mb: 4_096,
        },
        BuiltinModelDef {
            manifest: ModelManifest {
                id: "gemma4-e2b".to_string(),
                version: "1.0".to_string(),
                file_name: "gemma-4-E2B_q4_0-it.qat.gguf".to_string(),
                sha256: "fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634"
                    .to_string(),
                quant: "qat_q4_0".to_string(),
            },
            display_name: "Gemma4-E2B",
            // Official Gemma 4 model card (uniform across the family).
            temperature: Some(1.0),
            top_p: Some(0.95),
            top_k: Some(64),
            min_p: None,
            repeat_penalty: None,
            hf_repo: "google/gemma-4-E2B-it-qat-q4_0-gguf",
            hf_file: "gemma-4-E2B_q4_0-it.gguf",
            size_bytes: 3_100_000_000,
            // 128K default (2026-10-10): 5:1 sliding-window arch — 28
            // local layers pin at window 512 (28L × 512 × 1kv × 256hd × 2
            // × 2B ≈ 14 MB const), 7 global L × 1kv × 512 global_hd × 2 ×
            // 2B = 14,336 B/token → only ~1.9 GB KV at 128K. Cheapest big
            // window in the registry.
            default_ctx: 131072,
            max_ctx: 131072,
            kv_bytes_per_token: Some(14_336),
            kv_const_bytes: Some(14_680_064),
            notes: "Google 官方 QAT 量化 — 可挂 mmproj 加视觉",
            recommended: false,
            default_thinking: true,
            thinking_is_integral: false,
            min_ram_mb: 4_608,
        },
        BuiltinModelDef {
            manifest: ModelManifest {
                id: "ling30-tiny".to_string(),
                version: "1.0".to_string(),
                file_name: "ling-3.0-tiny-q4_k_m.gguf".to_string(),
                sha256: "9842cce7c1a07ad4adefd2b79a1035710ff196576d89128eade29351b79c8e68"
                    .to_string(),
                quant: "q4_k_m".to_string(),
            },
            display_name: "Ling-3.0-tiny",
            // Official inclusionAI card (thinking enabled by default).
            temperature: Some(1.0),
            top_p: Some(0.95),
            top_k: Some(20),
            min_p: None,
            repeat_penalty: None,
            hf_repo: "inclusionAI/Ling-3.0-tiny-GGUF",
            hf_file: "Ling-3.0-tiny-Q4_K_M.gguf",
            size_bytes: 4_823_894_880,
            // 128K default (2026-10-10 product decision, overriding the
            // 2026-09 eval: scores cliff past 8K, 62→48 @16K — long
            // sessions WILL degrade; the user accepted this after the
            // other models went 128K). Arch: bailing_hybrid, 3:1 KDA/MLA
            // blocks (config 24L → ~18 linear + 6 MLA), MoE 7.9B/1.3B
            // active. KV guard cost = WORST-CASE uncompressed MLA (6L ×
            // 16kv × 128hd × 2 × 2B = 49,152 B/token → 6.4 GB @128K);
            // llama.cpp may store the compressed latent (≈0.9 GB) — the
            // guard then over-derates small hosts, which is the safe
            // direction. KDA recurrence state ~1 MB/layer → 16 MiB const.
            // Official ceiling: 128K native, 256K via YaRN (needs
            // --rope-scaling, which we don't pass → max_ctx stays native).
            default_ctx: 131072,
            max_ctx: 131072,
            kv_bytes_per_token: Some(49_152),
            kv_const_bytes: Some(16_777_216),
            notes: "高速 MoE(1.3B 激活) — 默认 128K;注意 2026-09 评测 16K+ 有质量滑坡,长会话需观察;需 llama.cpp ≥ b10545",
            recommended: false,
            default_thinking: true,
            thinking_is_integral: true,
            min_ram_mb: 6_144,
        },
    ]
});

/// Registry lookup by id.
pub fn model_def(id: &str) -> Option<&'static BuiltinModelDef> {
    BUILTIN_MODELS.iter().find(|d| d.manifest.id == id)
}

/// The default (recommended) registry entry.
pub fn default_model_def() -> &'static BuiltinModelDef {
    BUILTIN_MODELS
        .iter()
        .find(|d| d.manifest.id == BUILTIN_MODEL_ID)
        .expect("default model present")
}

fn manifest_path(models_dir: &Path, id: &str) -> PathBuf {
    models_dir.join(id).join("manifest.json")
}

pub fn save_manifest(models_dir: &Path, m: &ModelManifest) -> anyhow::Result<()> {
    let dir = models_dir.join(&m.id);
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(m)?;
    std::fs::write(manifest_path(models_dir, &m.id), json)?;
    Ok(())
}

pub fn load_manifest(models_dir: &Path, id: &str) -> anyhow::Result<Option<ModelManifest>> {
    let p = manifest_path(models_dir, id);
    if !p.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(p)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "neomind-builtin-manifest-{}-{}",
            tag,
            std::process::id()
        ))
    }

    /// Every registry entry must carry an explicit sampling point — the
    /// spawn path writes these into llama-server defaults and the backend
    /// instance, and an accidental None would silently fall back to the
    /// legacy global (0.6/0.85/20) for a model whose official point differs.
    #[test]
    fn builtin_models_carry_sampling_points() {
        for def in BUILTIN_MODELS.iter() {
            assert!(
                def.temperature.is_some(),
                "{} missing temperature",
                def.manifest.id
            );
            assert!(def.top_p.is_some(), "{} missing top_p", def.manifest.id);
            assert!(def.top_k.is_some(), "{} missing top_k", def.manifest.id);
        }
    }

    /// The registry is hand-maintained — lock the official/measured
    /// points so a typo can't silently shift a model's behavior.
    #[test]
    fn builtin_models_sampling_values_locked() {
        let by_id = |id: &str| {
            BUILTIN_MODELS
                .iter()
                .find(|d| d.manifest.id == id)
                .unwrap_or_else(|| panic!("{id} missing from registry"))
        };
        let minicpm = by_id("minicpm5-2b");
        assert_eq!(
            (minicpm.temperature, minicpm.top_p, minicpm.top_k),
            (Some(1.0), Some(0.95), Some(40))
        );
        // The anti-repetition pair is a REQUIRED part of MiniCPM5's
        // official point (OpenBMB issue #374) — lock it separately so a
        // refactor can't drop the fields silently.
        assert_eq!(
            (minicpm.min_p, minicpm.repeat_penalty),
            (Some(0.0), Some(1.05))
        );
        let lfm = by_id("lfm25-2.6b");
        assert_eq!(
            (lfm.temperature, lfm.top_p, lfm.top_k),
            (Some(0.1), Some(1.0), Some(50))
        );
        assert_eq!(lfm.repeat_penalty, Some(1.1));
        assert_eq!(lfm.min_p, None);
        let qwen = by_id("qwen3.5-4b");
        assert_eq!(
            (qwen.temperature, qwen.top_p, qwen.top_k),
            (Some(0.7), Some(0.8), Some(20))
        );
        let gemma = by_id("gemma4-e2b");
        assert_eq!(
            (gemma.temperature, gemma.top_p, gemma.top_k),
            (Some(1.0), Some(0.95), Some(64))
        );
        let ling = by_id("ling30-tiny");
        assert_eq!(
            (ling.temperature, ling.top_p, ling.top_k),
            (Some(1.0), Some(0.95), Some(20))
        );
    }

    #[test]
    fn manifest_roundtrip() {
        let dir = temp_dir("roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        let m = ModelManifest {
            id: BUILTIN_MODEL_ID.to_string(),
            version: "1.0".to_string(),
            file_name: "lfm25-2.6b-q4_k_m.gguf".to_string(),
            sha256: "abc123".to_string(),
            quant: "q4_k_m".to_string(),
        };
        save_manifest(&dir, &m).expect("save");
        let loaded = load_manifest(&dir, BUILTIN_MODEL_ID)
            .expect("load")
            .expect("some");
        assert_eq!(loaded.file_name, m.file_name);
        assert_eq!(loaded.sha256, "abc123");
        assert_eq!(
            m.model_path(&dir),
            dir.join(BUILTIN_MODEL_ID).join("lfm25-2.6b-q4_k_m.gguf")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_missing_is_none() {
        let dir = temp_dir("missing");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_manifest(&dir, BUILTIN_MODEL_ID).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
