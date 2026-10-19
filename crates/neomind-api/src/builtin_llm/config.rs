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
        c
    }

    /// Effective context for a model whose own default is `default_ctx`:
    /// an explicit override wins, else the model default.
    pub fn effective_ctx(&self, default_ctx: u32) -> usize {
        self.ctx.unwrap_or(default_ctx as usize)
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
        // Companion defaults: on, on its own port clear of 29375/29376.
        assert!(c.vlm_enabled);
        assert_eq!(c.vlm_port, 29385);
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
        std::env::remove_var("NEOMIND_BUILTIN_VLM");
        std::env::remove_var("NEOMIND_BUILTIN_VLM_PORT");
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
}
