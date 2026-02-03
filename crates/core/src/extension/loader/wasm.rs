//! WASM extension loader for .wasm files.

use std::path::{Path, PathBuf};

use crate::extension::types::{ExtensionError, ExtensionMetadata, ExtensionType, Result};

/// Loader for WASM extensions (.wasm).
pub struct WasmExtensionLoader {
    /// Loaded WASM modules (kept for reference)
    _modules: Vec<String>,
}

impl WasmExtensionLoader {
    /// Create a new WASM extension loader.
    pub fn new() -> Self {
        Self {
            _modules: Vec::new(),
        }
    }

    /// Load extension metadata from a WASM file.
    ///
    /// Looks for a sidecar JSON file with the same name for metadata,
    /// or extracts metadata from WASM custom sections.
    pub async fn load(&self, path: &Path) -> Result<ExtensionMetadata> {
        // Validate file exists
        if !path.exists() {
            return Err(ExtensionError::NotFound(path.display().to_string()));
        }

        // Validate extension
        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            return Err(ExtensionError::InvalidFormat(
                "Not a WASM file".to_string(),
            ));
        }

        // Try to load metadata from sidecar JSON file
        let json_path = path.with_extension("json");
        if json_path.exists()
            && let Ok(content) = std::fs::read_to_string(&json_path)
            && let Ok(meta) = serde_json::from_str::<WasmMetadataJson>(&content) {
                return Ok(ExtensionMetadata::new(
                    &meta.id,
                    &meta.name,
                    semver::Version::parse(&meta.version).unwrap_or(semver::Version::new(1, 0, 0)),
                    ExtensionType::from_string(&meta.extension_type),
                )
                .with_file_path(path.to_path_buf()));
            }

        // Fall back to generating metadata from filename
        let file_name = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let metadata = ExtensionMetadata::new(
            file_name,
            format!("{} WASM Extension", file_name),
            semver::Version::new(1, 0, 0),
            ExtensionType::Generic,
        )
        .with_file_path(path.to_path_buf());

        Ok(metadata)
    }

    /// Discover WASM extensions in a directory.
    pub fn discover(&self, dir: &Path) -> Vec<PathBuf> {
        let mut extensions = Vec::new();

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if super::is_wasm_extension(&path) {
                    extensions.push(path);
                }
            }
        }

        extensions
    }
}

impl Default for WasmExtensionLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Sidecar JSON metadata format for WASM extensions.
#[derive(Debug, serde::Deserialize)]
struct WasmMetadataJson {
    id: String,
    name: String,
    version: String,
    #[serde(default = "default_extension_type")]
    extension_type: String,
}

fn default_extension_type() -> String {
    "generic".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_creation() {
        let loader = WasmExtensionLoader::new();
        assert!(loader._modules.is_empty());
    }
}
