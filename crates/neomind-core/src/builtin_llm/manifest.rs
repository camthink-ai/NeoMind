use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Default builtin model. `BUILTIN_MODELS` (below) is the full registry; the
/// user picks which one to install, this is the default/recommended entry.
pub const BUILTIN_MODEL_ID: &str = "lfm25-2.6b";

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
    /// Default context window for this model (KV size differs a lot — LFM's
    /// hybrid arch is cheap at 128K; Qwen/Gemma should run at 32K).
    pub default_ctx: u32,
    /// Native context ceiling (display); default_ctx is the run default.
    pub max_ctx: u32,
    /// Per-model sampling defaults (None = platform-wide legacy default).
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    /// One-line capability note for the picker.
    pub notes: &'static str,
    /// This entry is a fallback if the model cannot run (reserved).
    pub recommended: bool,
    /// Whether thinking/reasoning is enabled by default for this model.
    /// LFM's thinking is integral (cannot be off); Qwen3.5 runs faster
    /// non-thinking for agent tool calls (eval 76% cmd_ok was non-thinking);
    /// Gemma defaults to thinking.
    pub default_thinking: bool,
    /// Recommended MINIMUM available RAM (MB): weights + KV cache + runtime
    /// + OS headroom. The UI discourages (not blocks) installs below this.
    pub min_ram_mb: u64,
}

/// The builtin model registry. LFM2.5 is the default (small + cheap KV);
/// Qwen3.5-4B is the strongest edge agent per our 30-case evals; Gemma4-E2B
/// adds a QAT + vision-friendly option.
pub static BUILTIN_MODELS: LazyLock<Vec<BuiltinModelDef>> = LazyLock::new(|| {
    vec![
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
            // Measured-best (154-case A/B beat the official 0.1 card values).
            temperature: Some(0.6),
            top_p: Some(0.85),
            top_k: Some(20),
            hf_repo: "LiquidAI/LFM2.5-2.6B-GGUF",
            hf_file: "LFM2.5-2.6B-QAD-Q4_0.gguf",
            size_bytes: 1_500_000_000,
            default_ctx: 131072,
            max_ctx: 131072,
            notes: "小体积 + 原生 128K 上下文(hybrid KV 很省) — 资源紧张设备的默认选择",
            recommended: true,
            default_thinking: true,
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
            hf_repo: "unsloth/Qwen3.5-4B-GGUF",
            hf_file: "Qwen3.5-4B-Q4_K_M.gguf",
            size_bytes: 2_740_000_000,
            default_ctx: 32768,
            max_ctx: 262144,
            notes: "30 案 eval 最强的端侧 agent 模型(76% cmd_ok) — 工具调用首选",
            recommended: false,
            default_thinking: false,
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
            hf_repo: "google/gemma-4-E2B-it-qat-q4_0-gguf",
            hf_file: "gemma-4-E2B_q4_0-it.gguf",
            size_bytes: 3_100_000_000,
            default_ctx: 32768,
            max_ctx: 131072,
            notes: "Google 官方 QAT 量化 — 可挂 mmproj 加视觉",
            recommended: false,
            default_thinking: true,
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
            hf_repo: "bloomer010/Ling-3.0-tiny-GGUF",
            hf_file: "Ling-3.0-tiny-Q4_K_M.gguf",
            size_bytes: 4_823_894_880,
            // Native 128K; the run default keeps the full window (MoE KV is
            // modest) but the 6 GB floor steers small devices away.
            default_ctx: 131072,
            max_ctx: 131072,
            notes: "社区验证的 tiny MoE — 生成快,agent 得分强(77% 平 Qwen)",
            recommended: false,
            default_thinking: true,
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

    /// The registry is hand-maintained — lock the four official/measured
    /// points so a typo can't silently shift a model's behavior.
    #[test]
    fn builtin_models_sampling_values_locked() {
        let by_id = |id: &str| {
            BUILTIN_MODELS
                .iter()
                .find(|d| d.manifest.id == id)
                .unwrap_or_else(|| panic!("{id} missing from registry"))
        };
        let lfm = by_id("lfm25-2.6b");
        assert_eq!(
            (lfm.temperature, lfm.top_p, lfm.top_k),
            (Some(0.6), Some(0.85), Some(20))
        );
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
