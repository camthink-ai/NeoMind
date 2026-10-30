//! Builtin LLM bootstrap configuration (env-driven).

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BuiltinConfig {
    pub enabled: bool,
    pub port: u16,
    /// Context-size OVERRIDE. `None` = per-model default (LFM 128K native,
    /// Qwen/Gemma 32K). Set via NEOMIND_BUILTIN_LLM_CTX or the restart API's
    /// `ctx` param — the spawn sites honor this; previously the field existed
    /// but was silently ignored at spawn.
    pub ctx: Option<usize>,
    /// GPU layer override (`NEOMIND_BUILTIN_MODEL_NGL`). `None` (default):
    /// the spawn path auto-sizes the offload to the free VRAM on discrete
    /// NVIDIA GPUs (see `vram.rs` — CUDA-OOM-safe on shared GPUs, full
    /// offload on empty ones, CPU-only `-ngl 0` on saturated ones); on
    /// CPU/Jetson hosts `None` keeps llama-server's own default. An
    /// explicit value always wins and is never second-guessed.
    pub ngl: Option<u16>,
    pub model_path: Option<PathBuf>,
    pub quant_override: Option<String>,
    /// KV cache quantization at spawn: `true` (default) passes
    /// `-fa on -ctk q8_0 -ctv q8_0` — near-lossless, roughly halves KV
    /// memory (verified: 32K + q8 KV uses less device memory than 16K +
    /// f16 on the same model). `NEOMIND_BUILTIN_LLM_KV=f16|off|0` disables
    /// for runtimes/arches where FA or q8 KV misbehaves.
    pub kv_quant: bool,
    /// llama-server slot count. Default `Some(1)`; `NEOMIND_BUILTIN_LLM_PARALLEL=0`
    /// omits the flag (upstream default: 4 slots — the platform then budgets
    /// n_ctx ÷ slots per request). Values 1-16 set the slot count explicitly.
    pub parallel: Option<usize>,
    /// Vision-companion (VLM) feature switch: `NEOMIND_BUILTIN_VLM=off`
    /// disables the companion entirely (no spawn, no status block content,
    /// download of vlm entries still works on request).
    pub vlm_enabled: bool,
    /// Vision-companion llama-server port. Default 29385 — deliberately clear
    /// of 29375 (main builtin) and 29376 (a hot manual-VLM port seen in the
    /// field on Jetson boxes). `NEOMIND_BUILTIN_VLM_PORT` overrides.
    pub vlm_port: u16,
    /// Companion ctx OVERRIDE (`NEOMIND_BUILTIN_VLM_CTX`). `None` = the
    /// companion follows the SHARED ctx override / its model default — the
    /// pre-split behavior where the main-model env also moved the companion.
    pub vlm_ctx: Option<usize>,
    /// Companion KV-quant override (`NEOMIND_BUILTIN_VLM_KV`); None = follow
    /// the shared `kv_quant`.
    pub vlm_kv_quant: Option<bool>,
    /// Companion GPU-layer override (`NEOMIND_BUILTIN_VLM_NGL`); None =
    /// follow the shared `ngl` (and thus the same VRAM auto-sizing).
    pub vlm_ngl: Option<u16>,
}

impl Default for BuiltinConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            // [port choice] llama-server's OWN default is 8080 and 8081 is a
            // hot dev port — spawning there collides with any llama.cpp /
            // web-dev tooling the user already runs, and our port-conflict
            // guard then kill_process_on_port()s an innocent process.
            // 29375: deliberately obscure ("2" + the platform's 9375),
            // clears every AI-tool default (llama.cpp 8080, Ollama 11434,
            // LM Studio 1234, gradio 7860) and no known registered service.
            port: 29375,
            ctx: None,
            ngl: None,
            model_path: None,
            quant_override: None,
            kv_quant: true,
            parallel: Some(1),
            vlm_enabled: true,
            vlm_port: 29385,
            vlm_ctx: None,
            vlm_kv_quant: None,
            vlm_ngl: None,
        }
    }
}

impl BuiltinConfig {
    pub fn from_env() -> Self {
        let mut c = Self::default();
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_LLM") {
            if v.trim().eq_ignore_ascii_case("off") {
                c.enabled = false;
            }
        }
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_LLM_PORT") {
            if let Ok(p) = v.trim().parse() {
                c.port = p;
            }
        }
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_LLM_CTX") {
            // 0 / garbage is ignored rather than bricking the spawn.
            if let Ok(n) = v.trim().parse::<usize>() {
                if n >= 1024 {
                    c.ctx = Some(n);
                }
            }
        }
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_MODEL_PATH") {
            if !v.trim().is_empty() {
                c.model_path = Some(PathBuf::from(v.trim()));
            }
        }
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_MODEL_NGL") {
            if let Ok(n) = v.trim().parse() {
                c.ngl = Some(n);
            }
        }
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_LLM_PARALLEL") {
            match v.trim().parse::<usize>() {
                Ok(0) => c.parallel = None,
                Ok(n) if (1..=16).contains(&n) => c.parallel = Some(n),
                _ => {} // garbage ignored — keep the default
            }
        }
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_LLM_KV") {
            let v = v.trim().to_ascii_lowercase();
            if matches!(v.as_str(), "f16" | "off" | "0" | "false") {
                c.kv_quant = false;
            }
        }
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_VLM") {
            let v = v.trim().to_ascii_lowercase();
            if matches!(v.as_str(), "off" | "0" | "false" | "disable" | "disabled") {
                c.vlm_enabled = false;
            }
        }
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_VLM_PORT") {
            if let Ok(p) = v.trim().parse() {
                c.vlm_port = p;
            }
        }
        // Companion-specific engine overrides — the DECOUPLING matters more
        // than the knobs themselves: without these, the main-model
        // NEOMIND_BUILTIN_LLM_CTX/KV/NGL silently moved the companion too.
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_VLM_CTX") {
            if let Ok(n) = v.trim().parse::<usize>() {
                if n >= 1024 {
                    c.vlm_ctx = Some(n);
                }
            }
        }
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_VLM_KV") {
            let v = v.trim().to_ascii_lowercase();
            if matches!(v.as_str(), "f16" | "off" | "0" | "false") {
                c.vlm_kv_quant = Some(false);
            } else if matches!(v.as_str(), "q8_0" | "on" | "1" | "true") {
                c.vlm_kv_quant = Some(true);
            }
        }
        if let Ok(v) = std::env::var("NEOMIND_BUILTIN_VLM_NGL") {
            if let Ok(n) = v.trim().parse() {
                c.vlm_ngl = Some(n);
            }
        }
        apply_persisted_overrides(&mut c);
        c
    }

    /// Effective context for a model whose own default is `default_ctx`:
    /// an explicit override wins, else the model default.
    pub fn effective_ctx(&self, default_ctx: u32) -> usize {
        self.ctx.unwrap_or(default_ctx as usize)
    }
}

/// Engine knobs set through the restart API, persisted beside the model data.
///
/// Why: the API used to write only the PROCESS env; a service restart
/// re-applied the systemd drop-in and silently reverted the change
/// (2026-10-30: ctx 32768 -> 8192, below the agent's prompt overhead, every
/// chat turn failed). Precedence: default < env < these overrides. The UI
/// owns these values; clearing a field removes the override.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct EngineOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ngl: Option<u16>,
    /// Some(0) = omit `--parallel` (upstream default); Some(1..=16) = slots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_quant: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vlm_ctx: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vlm_kv_quant: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vlm_ngl: Option<u16>,
}

static OVERRIDES_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
static OVERRIDES_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Point [`BuiltinConfig::from_env`] at the persisted overrides file. Called
/// once at server startup; callers without a data dir (tests, subcommands)
/// keep the env-only behavior.
pub fn set_engine_overrides_path(path: std::path::PathBuf) {
    let _ = OVERRIDES_PATH.set(path);
}

pub fn engine_overrides_path() -> Option<std::path::PathBuf> {
    OVERRIDES_PATH.get().cloned()
}

impl EngineOverrides {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let tmp = path.with_file_name(format!(
            "{}.tmp",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("builtin_engine.json")
        ));
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    }

    fn apply_to(&self, c: &mut BuiltinConfig) {
        if let Some(v) = self.ctx {
            if v >= 1024 {
                c.ctx = Some(v);
            }
        }
        if let Some(v) = self.ngl {
            c.ngl = Some(v);
        }
        if let Some(v) = self.parallel {
            c.parallel = match v {
                0 => None,
                1..=16 => Some(v),
                _ => c.parallel,
            };
        }
        if let Some(v) = self.kv_quant {
            c.kv_quant = v;
        }
        if let Some(v) = self.vlm_ctx {
            if v >= 1024 {
                c.vlm_ctx = Some(v);
            }
        }
        if let Some(v) = self.vlm_kv_quant {
            c.vlm_kv_quant = Some(v);
        }
        if let Some(v) = self.vlm_ngl {
            c.vlm_ngl = Some(v);
        }
    }
}

fn apply_persisted_overrides(c: &mut BuiltinConfig) {
    let Some(path) = OVERRIDES_PATH.get() else {
        return;
    };
    let overrides = EngineOverrides::load(path);
    if overrides == EngineOverrides::default() {
        return;
    }
    overrides.apply_to(c);
    if !OVERRIDES_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::info!(
            ctx = ?c.ctx,
            ngl = ?c.ngl,
            parallel = ?c.parallel,
            kv_quant = c.kv_quant,
            "builtin llm: persisted engine overrides applied (win over systemd/process env)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    /// 环境变量是进程全局的,并行测试会互相踩;用模块级锁串行化。
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }
    #[serial_test::serial]
    #[test]
    fn default_config_has_expected_values() {
        let _g = env_lock().lock().unwrap();
        for var in [
            "NEOMIND_BUILTIN_LLM",
            "NEOMIND_BUILTIN_LLM_PORT",
            "NEOMIND_BUILTIN_LLM_CTX",
            "NEOMIND_BUILTIN_MODEL_PATH",
            "NEOMIND_BUILTIN_MODEL_NGL",
            "NEOMIND_BUILTIN_LLM_KV",
            "NEOMIND_BUILTIN_LLM_PARALLEL",
            "NEOMIND_BUILTIN_VLM",
            "NEOMIND_BUILTIN_VLM_PORT",
            "NEOMIND_BUILTIN_VLM_CTX",
            "NEOMIND_BUILTIN_VLM_KV",
            "NEOMIND_BUILTIN_VLM_NGL",
        ] {
            std::env::remove_var(var);
        }
        let c = BuiltinConfig::from_env();
        assert!(c.enabled);
        assert_eq!(c.port, 29375);
        assert_eq!(c.ctx, None);
        assert!(c.ngl.is_none());
        assert!(c.model_path.is_none());
        assert!(c.quant_override.is_none());
        // KV q8_0 + FA is the spawn default (upstream practice).
        assert!(c.kv_quant);
        // Single slot is the shipped default (see the config field's doc).
        assert_eq!(c.parallel, Some(1));
        // Companion defaults: on, on its own port clear of 29375/29376, and
        // with NO dedicated engine overrides (follow the shared knobs).
        assert!(c.vlm_enabled);
        assert_eq!(c.vlm_port, 29385);
        assert_eq!(c.vlm_ctx, None);
        assert_eq!(c.vlm_kv_quant, None);
        assert_eq!(c.vlm_ngl, None);
        // Effective ctx falls back to the model default.
        assert_eq!(c.effective_ctx(32_768), 32_768);
    }

    #[serial_test::serial]
    #[test]
    fn vlm_env_knobs() {
        let _g = env_lock().lock().unwrap();
        for v in ["off", "0", "false", "OFF"] {
            std::env::set_var("NEOMIND_BUILTIN_VLM", v);
            assert!(!BuiltinConfig::from_env().vlm_enabled, "{v:?} must disable");
        }
        for v in ["on", "1", ""] {
            std::env::set_var("NEOMIND_BUILTIN_VLM", v);
            assert!(BuiltinConfig::from_env().vlm_enabled, "{v:?} keeps default");
        }
        std::env::set_var("NEOMIND_BUILTIN_VLM_PORT", "30000");
        assert_eq!(BuiltinConfig::from_env().vlm_port, 30000);
        std::env::set_var("NEOMIND_BUILTIN_VLM_PORT", "garbage");
        assert_eq!(
            BuiltinConfig::from_env().vlm_port,
            29385,
            "garbage ignored — default stands"
        );
        // Dedicated companion overrides parse; None (unset) means "follow the
        // shared knob".
        std::env::set_var("NEOMIND_BUILTIN_VLM_CTX", "8192");
        assert_eq!(BuiltinConfig::from_env().vlm_ctx, Some(8192));
        std::env::set_var("NEOMIND_BUILTIN_VLM_CTX", "512");
        assert_eq!(
            BuiltinConfig::from_env().vlm_ctx,
            None,
            "sub-1024 ctx is ignored, same floor as the main knob"
        );
        std::env::set_var("NEOMIND_BUILTIN_VLM_KV", "f16");
        assert_eq!(BuiltinConfig::from_env().vlm_kv_quant, Some(false));
        std::env::set_var("NEOMIND_BUILTIN_VLM_KV", "q8_0");
        assert_eq!(BuiltinConfig::from_env().vlm_kv_quant, Some(true));
        std::env::set_var("NEOMIND_BUILTIN_VLM_NGL", "99");
        assert_eq!(BuiltinConfig::from_env().vlm_ngl, Some(99));
        for var in [
            "NEOMIND_BUILTIN_VLM",
            "NEOMIND_BUILTIN_VLM_PORT",
            "NEOMIND_BUILTIN_VLM_CTX",
            "NEOMIND_BUILTIN_VLM_KV",
            "NEOMIND_BUILTIN_VLM_NGL",
        ] {
            std::env::remove_var(var);
        }
    }

    #[serial_test::serial]
    #[test]
    fn kv_quant_env_kill_switch() {
        let _g = env_lock().lock().unwrap();
        for v in ["f16", "off", "0", "false", "F16"] {
            std::env::set_var("NEOMIND_BUILTIN_LLM_KV", v);
            assert!(
                !BuiltinConfig::from_env().kv_quant,
                "value {v:?} must disable"
            );
        }
        for v in ["q8_0", "on", "1", ""] {
            std::env::set_var("NEOMIND_BUILTIN_LLM_KV", v);
            assert!(
                BuiltinConfig::from_env().kv_quant,
                "value {v:?} must keep the default"
            );
        }
        std::env::remove_var("NEOMIND_BUILTIN_LLM_KV");
    }

    #[serial_test::serial]
    #[test]
    fn parallel_env_knob() {
        let _g = env_lock().lock().unwrap();
        std::env::remove_var("NEOMIND_BUILTIN_LLM_PARALLEL");
        assert_eq!(BuiltinConfig::from_env().parallel, Some(1));
        std::env::set_var("NEOMIND_BUILTIN_LLM_PARALLEL", "4");
        assert_eq!(BuiltinConfig::from_env().parallel, Some(4));
        std::env::set_var("NEOMIND_BUILTIN_LLM_PARALLEL", "0");
        assert_eq!(
            BuiltinConfig::from_env().parallel,
            None,
            "0 = omit flag (upstream default)"
        );
        std::env::set_var("NEOMIND_BUILTIN_LLM_PARALLEL", "99");
        assert_eq!(
            BuiltinConfig::from_env().parallel,
            Some(1),
            "out of range ignored"
        );
        std::env::remove_var("NEOMIND_BUILTIN_LLM_PARALLEL");
    }
    #[serial_test::serial]
    #[test]
    fn env_overrides_apply() {
        let _g = env_lock().lock().unwrap();
        std::env::set_var("NEOMIND_BUILTIN_LLM", "off");
        std::env::set_var("NEOMIND_BUILTIN_LLM_PORT", "9001");
        std::env::set_var("NEOMIND_BUILTIN_LLM_CTX", "65536");
        std::env::set_var("NEOMIND_BUILTIN_MODEL_PATH", "/tmp/custom.gguf");
        std::env::set_var("NEOMIND_BUILTIN_MODEL_NGL", "99");
        let c = BuiltinConfig::from_env();
        assert!(!c.enabled);
        assert_eq!(c.port, 9001);
        assert_eq!(c.ctx, Some(65_536));
        assert_eq!(c.effective_ctx(32_768), 65_536);
        assert_eq!(
            c.model_path.as_deref(),
            Some(std::path::Path::new("/tmp/custom.gguf"))
        );
        assert_eq!(c.ngl, Some(99));
        for var in [
            "NEOMIND_BUILTIN_LLM",
            "NEOMIND_BUILTIN_LLM_PORT",
            "NEOMIND_BUILTIN_LLM_CTX",
            "NEOMIND_BUILTIN_MODEL_PATH",
            "NEOMIND_BUILTIN_MODEL_NGL",
        ] {
            std::env::remove_var(var);
        }
    }
    #[serial_test::serial]
    #[test]
    fn invalid_ctx_env_is_ignored() {
        let _g = env_lock().lock().unwrap();
        std::env::set_var("NEOMIND_BUILTIN_LLM_CTX", "0");
        assert_eq!(BuiltinConfig::from_env().ctx, None);
        std::env::set_var("NEOMIND_BUILTIN_LLM_CTX", "not-a-number");
        assert_eq!(BuiltinConfig::from_env().ctx, None);
        std::env::remove_var("NEOMIND_BUILTIN_LLM_CTX");
    }

    /// Persisted engine overrides round-trip and apply on top of the env
    /// defaults (the API-set values must survive a service restart).
    #[test]
    fn engine_overrides_roundtrip_and_apply() {
        let dir =
            std::env::temp_dir().join(format!("neomind-engine-overrides-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("builtin_engine.json");
        let overrides = EngineOverrides {
            ctx: Some(32_768),
            ngl: Some(8),
            parallel: Some(0),
            kv_quant: Some(false),
            ..Default::default()
        };
        overrides.save(&path).expect("save overrides");
        assert_eq!(EngineOverrides::load(&path), overrides);

        let mut cfg = BuiltinConfig::default();
        EngineOverrides::load(&path).apply_to(&mut cfg);
        assert_eq!(cfg.ctx, Some(32_768));
        assert_eq!(cfg.ngl, Some(8));
        assert_eq!(cfg.parallel, None, "0 = omit --parallel");
        assert!(!cfg.kv_quant);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
