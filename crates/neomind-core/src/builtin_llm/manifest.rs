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
    /// One-line capability note for the picker.
    pub notes: &'static str,
    /// This entry is a fallback if the model cannot run (reserved).
    pub recommended: bool,
}

/// The builtin model registry. LFM2.5 is the default (small + cheap KV);
/// Qwen3.5-4B is the strongest edge agent per our 30-case evals; Gemma4-E2B
/// adds a QAT + vision-friendly option.
pub static BUILTIN_MODELS: LazyLock<Vec<BuiltinModelDef>> = LazyLock::new(|| vec![
    BuiltinModelDef {
        manifest: ModelManifest {
            id: "lfm25-2.6b".to_string(),
            version: "1.0".to_string(),
            file_name: "lfm25-2.6b-q4_k_m.gguf".to_string(),
            sha256: "79fdf00351b46cf26f020aead28d01889886be87c55fa0eb907e6f9b00bfee14".to_string(),
            quant: "q4_k_m".to_string(),
        },
        display_name: "LFM2.5-2.6B",
        hf_repo: "LiquidAI/LFM2.5-2.6B-GGUF",
        hf_file: "LFM2.5-2.6B-Q4_K_M.gguf",
        size_bytes: 1_600_000_000,
        default_ctx: 131072,
        notes: "小体积 + 原生 128K 上下文(hybrid KV 很省) — 资源紧张设备的默认选择",
        recommended: true,
    },
    BuiltinModelDef {
        manifest: ModelManifest {
            id: "qwen3.5-4b".to_string(),
            version: "1.0".to_string(),
            file_name: "qwen3.5-4b-q4_k_m.gguf".to_string(),
            sha256: "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4".to_string(),
            quant: "q4_k_m".to_string(),
        },
        display_name: "Qwen3.5-4B",
        hf_repo: "unsloth/Qwen3.5-4B-GGUF",
        hf_file: "Qwen3.5-4B-Q4_K_M.gguf",
        size_bytes: 2_740_000_000,
        default_ctx: 32768,
        notes: "30 案 eval 最强的端侧 agent 模型(76% cmd_ok) — 工具调用首选",
        recommended: false,
    },
    BuiltinModelDef {
        manifest: ModelManifest {
            id: "gemma4-e2b".to_string(),
            version: "1.0".to_string(),
            file_name: "gemma-4-E2B_q4_0-it.qat.gguf".to_string(),
            sha256: "fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634".to_string(),
            quant: "qat_q4_0".to_string(),
        },
        display_name: "Gemma4-E2B",
        hf_repo: "google/gemma-4-E2B-it-qat-q4_0-gguf",
        hf_file: "gemma-4-E2B_q4_0-it.qat.gguf",
        size_bytes: 3_100_000_000,
        default_ctx: 32768,
        notes: "Google 官方 QAT 量化 — 可挂 mmproj 加视觉",
        recommended: false,
    },
]);

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
