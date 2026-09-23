//! Frontend Component Storage
//!
//! Filesystem-based storage for custom dashboard frontend components.
//! Each component is stored as a directory containing:
//! - `manifest.json` — component metadata
//! - `bundle.js` — compiled JavaScript bundle

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Error;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Size constraints for a frontend component on the dashboard grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeConstraints {
    #[serde(default = "default_min_w")]
    pub min_w: u32,
    #[serde(default = "default_min_h")]
    pub min_h: u32,
    #[serde(default = "default_default_w")]
    pub default_w: u32,
    #[serde(default = "default_default_h")]
    pub default_h: u32,
    #[serde(default = "default_max_w")]
    pub max_w: u32,
    #[serde(default = "default_max_h")]
    pub max_h: u32,
}

fn default_min_w() -> u32 {
    1
}
fn default_min_h() -> u32 {
    1
}
fn default_default_w() -> u32 {
    2
}
fn default_default_h() -> u32 {
    2
}
fn default_max_w() -> u32 {
    12
}
fn default_max_h() -> u32 {
    12
}

impl Default for SizeConstraints {
    fn default() -> Self {
        Self {
            min_w: 1,
            min_h: 1,
            default_w: 2,
            default_h: 2,
            max_w: 12,
            max_h: 12,
        }
    }
}

/// Component manifest stored as `manifest.json` inside the component directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentManifest {
    pub id: String,
    /// Supports i18n: `{"en": "Clock", "zh": "时钟"}`
    pub name: serde_json::Value,
    pub description: serde_json::Value,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default)]
    pub size_constraints: SizeConstraints,
    #[serde(default)]
    pub has_data_source: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_data_sources: Option<u32>,
    #[serde(default)]
    pub has_display_config: bool,
    #[serde(default)]
    pub has_actions: bool,
    #[serde(default)]
    pub has_device_binding: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub device_type_filter: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_config: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<String>>,
    /// Global variable name the bundle registers on `window`.
    pub global_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_name: Option<String>,
    #[serde(default)]
    pub installed_at: i64,
    /// Origin of the component: `"local"` (CLI/upload) or `"marketplace"`.
    /// `None` for legacy components installed before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

fn default_icon() -> String {
    "Box".to_string()
}
fn default_category() -> String {
    "custom".to_string()
}
fn default_version() -> String {
    "1.0.0".to_string()
}

/// Entry in the remote marketplace index (`index.json` on GitHub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketComponentEntry {
    pub id: String,
    pub name: serde_json::Value,
    pub description: serde_json::Value,
    pub icon: String,
    pub category: String,
    pub version: String,
    pub author: Option<String>,
    pub size_constraints: SizeConstraints,
    pub has_data_source: bool,
    pub max_data_sources: Option<u32>,
    pub has_display_config: bool,
    pub has_actions: bool,
    #[serde(default)]
    pub has_device_binding: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub device_type_filter: Vec<String>,
    /// URL to the component's `manifest.json` on GitHub.
    pub manifest_url: String,
    /// URL to the component's `bundle.js` on GitHub.
    pub bundle_url: String,
}

/// Top-level structure of the marketplace `index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketIndex {
    pub version: String,
    pub components: Vec<MarketComponentEntry>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

const MANIFEST_FILE: &str = "manifest.json";
const BUNDLE_FILE: &str = "bundle.js";

/// Filesystem-backed store for frontend dashboard components.
///
/// Layout on disk:
/// ```text
/// {base_dir}/
///   {component_id}/
///     manifest.json
///     bundle.js
/// ```
#[derive(Clone)]
pub struct FrontendComponentStore {
    base_dir: PathBuf,
}

impl FrontendComponentStore {
    /// Open (or create) the store at the given base directory.
    pub fn open(base_dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let base_dir = base_dir.into();
        fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    /// Install a component by writing its manifest and bundle bytes.
    pub fn install(&self, manifest: &ComponentManifest, bundle_bytes: &[u8]) -> Result<(), Error> {
        let dir = self.base_dir.join(&manifest.id);
        fs::create_dir_all(&dir)?;

        let manifest_json = serde_json::to_string_pretty(manifest)?;
        crate::atomic_write::write(&dir.join(MANIFEST_FILE), manifest_json)?;
        crate::atomic_write::write(&dir.join(BUNDLE_FILE), bundle_bytes)?;

        Ok(())
    }

    /// List all installed component manifests.
    pub fn list_all(&self) -> Result<Vec<ComponentManifest>, Error> {
        let mut manifests = Vec::new();

        let entries = match fs::read_dir(&self.base_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(manifests),
            Err(e) => return Err(e.into()),
        };

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join(MANIFEST_FILE);
            if !manifest_path.exists() {
                continue;
            }
            if let Some(manifest) = Self::read_manifest_file(&manifest_path)? {
                manifests.push(manifest);
            }
        }

        Ok(manifests)
    }

    /// Load the manifest for a specific component.
    ///
    /// Returns `Ok(None)` if the component directory or manifest does not exist.
    pub fn load_manifest(&self, id: &str) -> Result<Option<ComponentManifest>, Error> {
        let manifest_path = self.base_dir.join(id).join(MANIFEST_FILE);
        if !manifest_path.exists() {
            return Ok(None);
        }
        Self::read_manifest_file(&manifest_path)
    }

    /// Check whether a component with the given ID is installed.
    pub fn exists(&self, id: &str) -> bool {
        self.base_dir.join(id).join(MANIFEST_FILE).exists()
    }

    /// Get the filesystem path to a component's bundle file.
    ///
    /// Returns `None` if the component is not installed.
    pub fn get_bundle_path(&self, id: &str) -> Option<PathBuf> {
        let path = self.base_dir.join(id).join(BUNDLE_FILE);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// Delete a component (removes the entire component directory).
    pub fn delete(&self, id: &str) -> Result<(), Error> {
        let dir = self.base_dir.join(id);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    // -- helpers --

    fn read_manifest_file(path: &PathBuf) -> Result<Option<ComponentManifest>, Error> {
        let data = match fs::read_to_string(path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let manifest: ComponentManifest = serde_json::from_str(&data)?;
        Ok(Some(manifest))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> FrontendComponentStore {
        crate::prune_stale_temp_entries("neomind-test-fc-", std::time::Duration::from_secs(3600));
        let dir = std::env::temp_dir().join(format!("neomind-test-fc-{}", uuid::Uuid::new_v4()));
        FrontendComponentStore::open(&dir).expect("failed to open test store")
    }

    fn sample_manifest(id: &str) -> ComponentManifest {
        ComponentManifest {
            id: id.to_string(),
            name: serde_json::json!({"en": "Test Component", "zh": "测试组件"}),
            description: serde_json::json!({"en": "A test component"}),
            icon: "Box".to_string(),
            category: "custom".to_string(),
            version: "1.0.0".to_string(),
            author: Some("test-author".to_string()),
            size_constraints: SizeConstraints::default(),
            has_data_source: false,
            max_data_sources: None,
            has_display_config: false,
            has_actions: false,
            has_device_binding: false,
            device_type_filter: vec![],
            config_schema: None,
            default_config: None,
            variants: None,
            global_name: "TestComponent".to_string(),
            export_name: None,
            installed_at: 1700000000,
            source: None,
        }
    }

    #[test]
    fn test_install_and_load() {
        let store = test_store();
        let manifest = sample_manifest("clock");
        let bundle = b"// bundle content";

        store.install(&manifest, bundle).unwrap();

        // Manifest loads correctly
        let loaded = store
            .load_manifest("clock")
            .unwrap()
            .expect("manifest should exist");
        assert_eq!(loaded.id, "clock");
        assert_eq!(loaded.icon, "Box");
        assert_eq!(loaded.global_name, "TestComponent");

        // Bundle path resolves
        let bundle_path = store.get_bundle_path("clock").expect("bundle should exist");
        assert!(bundle_path.exists());
        assert_eq!(fs::read(&bundle_path).unwrap(), bundle);

        // exists check
        assert!(store.exists("clock"));
    }

    #[test]
    fn test_list_all() {
        let store = test_store();
        let m1 = sample_manifest("comp-a");
        let m2 = sample_manifest("comp-b");

        store.install(&m1, b"// a").unwrap();
        store.install(&m2, b"// b").unwrap();

        let all = store.list_all().unwrap();
        assert_eq!(all.len(), 2);

        let ids: Vec<&str> = all.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"comp-a"));
        assert!(ids.contains(&"comp-b"));
    }

    #[test]
    fn test_delete() {
        let store = test_store();
        let manifest = sample_manifest("to-delete");
        store.install(&manifest, b"// delete me").unwrap();

        assert!(store.exists("to-delete"));
        store.delete("to-delete").unwrap();
        assert!(!store.exists("to-delete"));
        assert!(store.load_manifest("to-delete").unwrap().is_none());
    }

    #[test]
    fn test_nonexistent() {
        let store = test_store();

        assert!(!store.exists("no-such-component"));
        assert!(store.load_manifest("no-such-component").unwrap().is_none());
        assert!(store.get_bundle_path("no-such-component").is_none());

        // Deleting a nonexistent component should succeed silently
        assert!(store.delete("no-such-component").is_ok());

        // list_all on empty store
        let all = store.list_all().unwrap();
        assert!(all.is_empty());
    }
}
