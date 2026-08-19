//! Builtin LLM bootstrap configuration (env-driven).

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BuiltinConfig {
    pub enabled: bool,
    pub port: u16,
    pub ctx: usize,
    pub ngl: Option<u16>,
    pub model_path: Option<PathBuf>,
    pub quant_override: Option<String>,
}

impl Default for BuiltinConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 8081,
            ctx: 131_072, // LFM2.5-2.6B 原生 128K,hybrid 架构 KV 很小(128K f16 ~3.7GB),2026-08-19 eval 定
            ngl: None,
            model_path: None,
            quant_override: None,
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
        c
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

    #[test]
    fn default_config_has_expected_values() {
        let _g = env_lock().lock().unwrap();
        for var in [
            "NEOMIND_BUILTIN_LLM",
            "NEOMIND_BUILTIN_LLM_PORT",
            "NEOMIND_BUILTIN_MODEL_PATH",
            "NEOMIND_BUILTIN_MODEL_NGL",
        ] {
            std::env::remove_var(var);
        }
        let c = BuiltinConfig::from_env();
        assert!(c.enabled);
        assert_eq!(c.port, 8081);
        assert_eq!(c.ctx, 131_072);
        assert!(c.ngl.is_none());
        assert!(c.model_path.is_none());
        assert!(c.quant_override.is_none());
    }

    #[test]
    fn env_overrides_apply() {
        let _g = env_lock().lock().unwrap();
        std::env::set_var("NEOMIND_BUILTIN_LLM", "off");
        std::env::set_var("NEOMIND_BUILTIN_LLM_PORT", "9001");
        std::env::set_var("NEOMIND_BUILTIN_MODEL_PATH", "/tmp/custom.gguf");
        std::env::set_var("NEOMIND_BUILTIN_MODEL_NGL", "99");
        let c = BuiltinConfig::from_env();
        assert!(!c.enabled);
        assert_eq!(c.port, 9001);
        assert_eq!(
            c.model_path.as_deref(),
            Some(std::path::Path::new("/tmp/custom.gguf"))
        );
        assert_eq!(c.ngl, Some(99));
        for var in [
            "NEOMIND_BUILTIN_LLM",
            "NEOMIND_BUILTIN_LLM_PORT",
            "NEOMIND_BUILTIN_MODEL_PATH",
            "NEOMIND_BUILTIN_MODEL_NGL",
        ] {
            std::env::remove_var(var);
        }
    }
}
