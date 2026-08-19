use std::path::{Path, PathBuf};

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
