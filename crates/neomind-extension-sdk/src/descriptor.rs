//! Plugin descriptor definition.
//!
//! This module provides the descriptor structure that defines a plugin's metadata.

/// Plugin ABI version (must match NeoMind core)
pub const PLUGIN_ABI_VERSION: u32 = 1;

/// Plugin type identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginType {
    /// LLM backend plugin
    LlmBackend,
    /// Device adapter plugin
    DeviceAdapter,
    /// Storage backend plugin
    StorageBackend,
    /// Tool plugin
    Tool,
    /// Integration plugin
    Integration,
    /// Alert channel plugin
    AlertChannel,
    /// Custom plugin type
    Custom(&'static str),
}

impl PluginType {
    /// Get the string representation of the plugin type
    pub fn as_str(&self) -> &str {
        match self {
            PluginType::LlmBackend => "llm_backend",
            PluginType::DeviceAdapter => "device_adapter",
            PluginType::StorageBackend => "storage_backend",
            PluginType::Tool => "tool",
            PluginType::Integration => "integration",
            PluginType::AlertChannel => "alert_channel",
            PluginType::Custom(s) => s,
        }
    }
}

/// Plugin descriptor builder
#[derive(Debug)]
pub struct PluginDescriptor {
    /// Plugin unique ID
    pub id: String,

    /// Plugin display name
    pub name: String,

    /// Plugin version (semver)
    pub version: String,

    /// Plugin type
    pub plugin_type: PluginType,

    /// Plugin description
    pub description: String,

    /// Required NeoMind version (semver requirement)
    pub required_neomind: String,

    /// Author name
    pub author: Option<String>,

    /// Homepage URL
    pub homepage: Option<String>,

    /// Repository URL
    pub repository: Option<String>,

    /// License
    pub license: Option<String>,

    /// Configuration schema (JSON string)
    pub config_schema: Option<String>,

    /// Plugin capabilities
    pub capabilities: u64,
}

impl PluginDescriptor {
    /// Create a new descriptor with required fields
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            version: version.into(),
            plugin_type: PluginType::Tool,
            description: String::new(),
            required_neomind: ">=1.0.0".to_string(),
            author: None,
            homepage: None,
            repository: None,
            license: None,
            config_schema: None,
            capabilities: 0,
            id,
        }
    }

    /// Set the plugin type
    pub fn with_plugin_type(mut self, plugin_type: PluginType) -> Self {
        self.plugin_type = plugin_type;
        self
    }

    /// Set the plugin name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the plugin description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the required NeoMind version
    pub fn with_required_neomind(mut self, version: impl Into<String>) -> Self {
        self.required_neomind = version.into();
        self
    }

    /// Set the author
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Set the homepage URL
    pub fn with_homepage(mut self, homepage: impl Into<String>) -> Self {
        self.homepage = Some(homepage.into());
        self
    }

    /// Set the repository URL
    pub fn with_repository(mut self, repository: impl Into<String>) -> Self {
        self.repository = Some(repository.into());
        self
    }

    /// Set the license
    pub fn with_license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    /// Set the configuration schema
    pub fn with_config_schema(mut self, schema: impl Into<String>) -> Self {
        self.config_schema = Some(schema.into());
        self
    }

    /// Add a capability flag
    pub fn with_capability(mut self, capability: u64) -> Self {
        self.capabilities |= capability;
        self
    }

    /// Export as C-compatible descriptor
    ///
    /// # Safety
    /// Returns pointers to static memory - these must outlive the plugin
    pub unsafe fn export(&self) -> CPluginDescriptor {
        CPluginDescriptor {
            abi_version: PLUGIN_ABI_VERSION,
            plugin_type: self.plugin_type.as_str().as_ptr(),
            plugin_type_len: self.plugin_type.as_str().len(),
            id: self.id.as_ptr(),
            id_len: self.id.len(),
            name: self.name.as_ptr(),
            name_len: self.name.len(),
            version: self.version.as_ptr(),
            version_len: self.version.len(),
            description: self.description.as_ptr(),
            description_len: self.description.len(),
            required_neomind: self.required_neomind.as_ptr(),
            required_neomind_len: self.required_neomind.len(),
            author: self
                .author
                .as_ref()
                .map_or(std::ptr::null(), |s| s.as_ptr()),
            author_len: self.author.as_ref().map_or(0, |s| s.len()),
            homepage: self
                .homepage
                .as_ref()
                .map_or(std::ptr::null(), |s| s.as_ptr()),
            homepage_len: self.homepage.as_ref().map_or(0, |s| s.len()),
            repository: self
                .repository
                .as_ref()
                .map_or(std::ptr::null(), |s| s.as_ptr()),
            repository_len: self.repository.as_ref().map_or(0, |s| s.len()),
            license: self
                .license
                .as_ref()
                .map_or(std::ptr::null(), |s| s.as_ptr()),
            license_len: self.license.as_ref().map_or(0, |s| s.len()),
            create_fn: create_fn_ptr as *const (),
            destroy_fn: destroy_fn_ptr as *const (),
            config_schema: self
                .config_schema
                .as_ref()
                .map_or(std::ptr::null(), |s| s.as_ptr()),
            config_schema_len: self.config_schema.as_ref().map_or(0, |s| s.len()),
            capabilities: self.capabilities,
        }
    }
}

/// C-compatible plugin descriptor
#[repr(C)]
pub struct CPluginDescriptor {
    pub abi_version: u32,
    pub plugin_type: *const u8,
    pub plugin_type_len: usize,
    pub id: *const u8,
    pub id_len: usize,
    pub name: *const u8,
    pub name_len: usize,
    pub version: *const u8,
    pub version_len: usize,
    pub description: *const u8,
    pub description_len: usize,
    pub required_neomind: *const u8,
    pub required_neomind_len: usize,
    pub author: *const u8,
    pub author_len: usize,
    pub homepage: *const u8,
    pub homepage_len: usize,
    pub repository: *const u8,
    pub repository_len: usize,
    pub license: *const u8,
    pub license_len: usize,
    pub create_fn: *const (),
    pub destroy_fn: *const (),
    pub config_schema: *const u8,
    pub config_schema_len: usize,
    pub capabilities: u64,
}

/// Plugin capability flags
pub mod capabilities {
    /// Plugin can be started and stopped
    pub const LIFECYCLE: u64 = 1 << 0;
    /// Plugin supports streaming responses
    pub const STREAMING: u64 = 1 << 1;
    /// Plugin supports async operations
    pub const ASYNC: u64 = 1 << 2;
    /// Plugin has its own configuration UI
    pub const HAS_CONFIG: u64 = 1 << 3;
    /// Plugin supports hot reload
    pub const HOT_RELOAD: u64 = 1 << 4;
    /// Plugin requires network access
    pub const NETWORK: u64 = 1 << 5;
    /// Plugin requires file system access
    pub const FILE_SYSTEM: u64 = 1 << 6;
    /// Plugin is thread-safe
    pub const THREAD_SAFE: u64 = 1 << 7;
}

/// Default create function (placeholder)
extern "C" fn create_fn_ptr(_config_json: *const u8, _config_len: usize) -> *mut () {
    std::ptr::null_mut()
}

/// Default destroy function (placeholder)
extern "C" fn destroy_fn_ptr(_instance: *mut ()) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor_builder() {
        let desc = PluginDescriptor::new("test-plugin", "1.0.0")
            .with_plugin_type(PluginType::Tool)
            .with_name("Test Plugin")
            .with_description("A test plugin")
            .with_author("Test Author");

        assert_eq!(desc.id, "test-plugin");
        assert_eq!(desc.name, "Test Plugin");
        assert_eq!(desc.version, "1.0.0");
        assert_eq!(desc.description, "A test plugin");
        assert_eq!(desc.author, Some("Test Author".to_string()));
    }

    #[test]
    fn test_plugin_type_as_str() {
        assert_eq!(PluginType::Tool.as_str(), "tool");
        assert_eq!(PluginType::LlmBackend.as_str(), "llm_backend");
        assert_eq!(PluginType::DeviceAdapter.as_str(), "device_adapter");
    }
}
