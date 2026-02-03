//! Extension registry for managing dynamically loaded extensions.
//!
//! The registry provides:
//! - Extension registration and lifecycle management
//! - Extension discovery from filesystem
//! - Health monitoring

use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

use super::loader::{NativeExtensionLoader, WasmExtensionLoader};
use super::types::{
    DynExtension, ExtensionError, ExtensionMetadata, ExtensionState, ExtensionStats,
    ExtensionType, Result,
};

/// Information about a registered extension.
#[derive(Debug, Clone)]
pub struct ExtensionInfo {
    /// Extension metadata
    pub metadata: ExtensionMetadata,
    /// Current state
    pub state: ExtensionState,
    /// Runtime statistics
    pub stats: ExtensionStats,
    /// When the extension was loaded
    pub loaded_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Registry for managing extensions.
pub struct ExtensionRegistry {
    /// Registered extensions
    extensions: RwLock<HashMap<String, DynExtension>>,
    /// Extension information cache
    info_cache: RwLock<HashMap<String, ExtensionInfo>>,
    /// Native extension loader
    native_loader: NativeExtensionLoader,
    /// WASM extension loader
    wasm_loader: WasmExtensionLoader,
    /// Extension directories to scan
    extension_dirs: Vec<PathBuf>,
}

impl ExtensionRegistry {
    /// Create a new extension registry.
    pub fn new() -> Self {
        Self {
            extensions: RwLock::new(HashMap::new()),
            info_cache: RwLock::new(HashMap::new()),
            native_loader: NativeExtensionLoader::new(),
            wasm_loader: WasmExtensionLoader::new(),
            extension_dirs: vec![],
        }
    }

    /// Add an extension directory to scan.
    pub fn add_extension_dir(&mut self, path: PathBuf) {
        self.extension_dirs.push(path);
    }

    /// Register an extension.
    pub async fn register(&self, extension: DynExtension) -> Result<()> {
        let ext = extension.read().await;
        let id = ext.metadata().id.clone();
        let metadata = ext.metadata().clone();
        let state = ext.state();
        let stats = ext.stats();
        drop(ext);

        // Check if already registered
        if self.extensions.read().await.contains_key(&id) {
            return Err(ExtensionError::AlreadyRegistered(id));
        }

        // Store extension
        self.extensions.write().await.insert(id.clone(), extension);

        // Store info
        self.info_cache.write().await.insert(
            id,
            ExtensionInfo {
                metadata,
                state,
                stats,
                loaded_at: Some(chrono::Utc::now()),
            },
        );

        Ok(())
    }

    /// Unregister an extension.
    pub async fn unregister(&self, id: &str) -> Result<()> {
        // Shutdown extension if running
        if let Some(ext) = self.extensions.read().await.get(id) {
            let mut ext = ext.write().await;
            let _ = ext.shutdown().await;
        }

        // Remove from registry
        self.extensions.write().await.remove(id);
        self.info_cache.write().await.remove(id);

        Ok(())
    }

    /// Get an extension by ID.
    pub async fn get(&self, id: &str) -> Option<DynExtension> {
        self.extensions.read().await.get(id).cloned()
    }

    /// Get extension info by ID.
    pub async fn get_info(&self, id: &str) -> Option<ExtensionInfo> {
        // Update info from actual extension state
        if let Some(ext) = self.extensions.read().await.get(id) {
            let ext = ext.read().await;
            let mut cache = self.info_cache.write().await;
            if let Some(info) = cache.get_mut(id) {
                info.state = ext.state();
                info.stats = ext.stats();
            }
        }
        self.info_cache.read().await.get(id).cloned()
    }

    /// List all extensions.
    pub async fn list(&self) -> Vec<ExtensionInfo> {
        // Update all info from actual extension states
        let extensions = self.extensions.read().await;
        let mut cache = self.info_cache.write().await;

        for (id, ext) in extensions.iter() {
            let ext = ext.read().await;
            if let Some(info) = cache.get_mut(id) {
                info.state = ext.state();
                info.stats = ext.stats();
            }
        }
        drop(extensions);

        cache.values().cloned().collect()
    }

    /// List extensions by type.
    pub async fn list_by_type(&self, ext_type: ExtensionType) -> Vec<ExtensionInfo> {
        self.list()
            .await
            .into_iter()
            .filter(|info| info.metadata.extension_type == ext_type)
            .collect()
    }

    /// Initialize an extension.
    pub async fn initialize(&self, id: &str, config: &serde_json::Value) -> Result<()> {
        let ext = self
            .get(id)
            .await
            .ok_or_else(|| ExtensionError::NotFound(id.to_string()))?;

        let mut ext = ext.write().await;
        ext.initialize(config).await?;

        // Update cache
        if let Some(info) = self.info_cache.write().await.get_mut(id) {
            info.state = ext.state();
        }

        Ok(())
    }

    /// Start an extension.
    pub async fn start(&self, id: &str) -> Result<()> {
        let ext = self
            .get(id)
            .await
            .ok_or_else(|| ExtensionError::NotFound(id.to_string()))?;

        let mut ext = ext.write().await;
        ext.start().await?;

        // Update cache
        if let Some(info) = self.info_cache.write().await.get_mut(id) {
            info.state = ext.state();
            info.stats = ext.stats();
        }

        Ok(())
    }

    /// Stop an extension.
    pub async fn stop(&self, id: &str) -> Result<()> {
        let ext = self
            .get(id)
            .await
            .ok_or_else(|| ExtensionError::NotFound(id.to_string()))?;

        let mut ext = ext.write().await;
        ext.stop().await?;

        // Update cache
        if let Some(info) = self.info_cache.write().await.get_mut(id) {
            info.state = ext.state();
            info.stats = ext.stats();
        }

        Ok(())
    }

    /// Perform health check on an extension.
    pub async fn health_check(&self, id: &str) -> Result<bool> {
        let ext = self
            .get(id)
            .await
            .ok_or_else(|| ExtensionError::NotFound(id.to_string()))?;

        let ext = ext.read().await;
        ext.health_check().await
    }

    /// Execute a command on an extension.
    pub async fn execute_command(
        &self,
        id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let ext = self
            .get(id)
            .await
            .ok_or_else(|| ExtensionError::NotFound(id.to_string()))?;

        let ext = ext.read().await;
        ext.handle_command(command, args).await
    }

    /// Load an extension from a file path.
    pub async fn load_from_path(&self, path: &PathBuf) -> Result<ExtensionMetadata> {
        let extension = path.extension().and_then(|e| e.to_str());

        match extension {
            Some("so") | Some("dylib") | Some("dll") => {
                self.native_loader.load(path).await
            }
            Some("wasm") => {
                self.wasm_loader.load(path).await
            }
            _ => Err(ExtensionError::InvalidFormat(format!(
                "Unsupported extension format: {:?}",
                path
            ))),
        }
    }

    /// Discover extensions in configured directories.
    pub async fn discover(&self) -> Vec<ExtensionMetadata> {
        let mut discovered = Vec::new();

        for dir in &self.extension_dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if is_extension_file(&path)
                        && let Ok(meta) = self.load_from_path(&path).await {
                            discovered.push(meta);
                        }
                }
            }
        }

        discovered
    }

    /// Get the number of registered extensions.
    pub async fn count(&self) -> usize {
        self.extensions.read().await.len()
    }

    /// Check if an extension is registered.
    pub async fn contains(&self, id: &str) -> bool {
        self.extensions.read().await.contains_key(id)
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a file is an extension file.
fn is_extension_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| matches!(ext, "so" | "dylib" | "dll" | "wasm"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = ExtensionRegistry::new();
        assert_eq!(registry.count().await, 0);
    }

    #[test]
    fn test_is_extension_file() {
        assert!(is_extension_file(&PathBuf::from("test.so")));
        assert!(is_extension_file(&PathBuf::from("test.dylib")));
        assert!(is_extension_file(&PathBuf::from("test.wasm")));
        assert!(!is_extension_file(&PathBuf::from("test.rs")));
        assert!(!is_extension_file(&PathBuf::from("test.txt")));
    }
}
