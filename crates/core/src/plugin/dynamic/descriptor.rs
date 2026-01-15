//! Dynamic plugin descriptor.
//!
//! This module defines the plugin descriptor structure that must be exported
//! by all dynamic plugins (.so/.dylib/.dll files).

use std::fmt::{self, Display, Formatter};

/// Current plugin ABI version.
/// Plugins must export a descriptor with this version to be loaded.
pub const PLUGIN_ABI_VERSION: u32 = 1;

/// Plugin descriptor exported by dynamic libraries.
///
/// Every dynamic plugin must export a symbol named `neotalk_plugin_descriptor`
/// that points to this structure.
#[repr(C)]
pub struct PluginDescriptor {
    /// ABI version - must match PLUGIN_ABI_VERSION
    pub abi_version: u32,

    /// Plugin type identifier (e.g., "llm_backend", "device_adapter", "tool")
    pub plugin_type: *const u8,
    pub plugin_type_len: usize,

    /// Plugin unique ID
    pub id: *const u8,
    pub id_len: usize,

    /// Plugin display name
    pub name: *const u8,
    pub name_len: usize,

    /// Plugin version (semver)
    pub version: *const u8,
    pub version_len: usize,

    /// Plugin description
    pub description: *const u8,
    pub description_len: usize,

    /// Required NeoTalk version (semver requirement)
    pub required_neotalk: *const u8,
    pub required_neotalk_len: usize,

    /// Author name
    pub author: *const u8,
    pub author_len: usize,

    /// Homepage URL
    pub homepage: *const u8,
    pub homepage_len: usize,

    /// Repository URL
    pub repository: *const u8,
    pub repository_len: usize,

    /// License
    pub license: *const u8,
    pub license_len: usize,

    /// Pointer to the create function
    pub create_fn: *const (),

    /// Pointer to the destroy function
    pub destroy_fn: *const (),

    /// Pointer to the config schema (JSON string)
    pub config_schema: *const u8,
    pub config_schema_len: usize,

    /// Plugin capabilities flags
    pub capabilities: u64,
}

/// Bit flags for plugin capabilities.
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PluginCapabilities: u64 {
        /// Plugin can be started and stopped
        const LIFECYCLE = 1 << 0;
        /// Plugin supports streaming responses
        const STREAMING = 1 << 1;
        /// Plugin supports async operations
        const ASYNC = 1 << 2;
        /// Plugin has its own configuration UI
        const HAS_CONFIG = 1 << 3;
        /// Plugin supports hot reload
        const HOT_RELOAD = 1 << 4;
        /// Plugin requires network access
        const NETWORK = 1 << 5;
        /// Plugin requires file system access
        const FILE_SYSTEM = 1 << 6;
        /// Plugin is thread-safe
        const THREAD_SAFE = 1 << 7;
    }
}

impl Default for PluginCapabilities {
    fn default() -> Self {
        Self::ASYNC | Self::THREAD_SAFE
    }
}

/// Parsed plugin descriptor with owned strings.
#[derive(Debug, Clone)]
pub struct ParsedPluginDescriptor {
    /// Plugin type identifier
    pub plugin_type: String,

    /// Plugin unique ID
    pub id: String,

    /// Plugin display name
    pub name: String,

    /// Plugin version
    pub version: String,

    /// Plugin description
    pub description: String,

    /// Required NeoTalk version
    pub required_neotalk: String,

    /// Author name
    pub author: Option<String>,

    /// Homepage URL
    pub homepage: Option<String>,

    /// Repository URL
    pub repository: Option<String>,

    /// License
    pub license: Option<String>,

    /// Configuration schema (JSON)
    pub config_schema: Option<serde_json::Value>,

    /// Plugin capabilities
    pub capabilities: PluginCapabilities,

    /// Create function pointer
    pub create_fn: *const (),

    /// Destroy function pointer
    pub destroy_fn: *const (),
}

impl ParsedPluginDescriptor {
    /// Parse a raw plugin descriptor.
    ///
    /// # Safety
    /// The raw descriptor pointers must be valid and point to null-terminated strings.
    pub unsafe fn from_raw(raw: &PluginDescriptor) -> Result<Self, DescriptorError> {
        // Check ABI version
        if raw.abi_version != PLUGIN_ABI_VERSION {
            return Err(DescriptorError::AbiMismatch {
                expected: PLUGIN_ABI_VERSION,
                found: raw.abi_version,
            });
        }

        let extract_string = |ptr: *const u8, len: usize| -> Option<String> {
            if ptr.is_null() || len == 0 {
                return None;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            String::from_utf8(slice.to_vec()).ok()
        };

        let extract_required_string = |ptr: *const u8, len: usize, field: &str| -> Result<String, DescriptorError> {
            if ptr.is_null() || len == 0 {
                return Err(DescriptorError::MissingField(field.to_string()));
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            String::from_utf8(slice.to_vec())
                .map_err(|e| DescriptorError::InvalidUtf8(field.to_string(), e))
        };

        let plugin_type = extract_required_string(raw.plugin_type, raw.plugin_type_len, "plugin_type")?;
        let id = extract_required_string(raw.id, raw.id_len, "id")?;
        let name = extract_required_string(raw.name, raw.name_len, "name")?;
        let version = extract_required_string(raw.version, raw.version_len, "version")?;
        let description = extract_string(raw.description, raw.description_len).unwrap_or_default();
        let required_neotalk = extract_string(raw.required_neotalk, raw.required_neotalk_len)
            .unwrap_or_else(|| ">=1.0.0".to_string());

        let config_schema = if !raw.config_schema.is_null() && raw.config_schema_len > 0 {
            let schema_str = extract_string(raw.config_schema, raw.config_schema_len).unwrap_or_default();
            serde_json::from_str(&schema_str).ok()
        } else {
            None
        };

        Ok(Self {
            plugin_type,
            id,
            name,
            version,
            description,
            required_neotalk,
            author: extract_string(raw.author, raw.author_len),
            homepage: extract_string(raw.homepage, raw.homepage_len),
            repository: extract_string(raw.repository, raw.repository_len),
            license: extract_string(raw.license, raw.license_len),
            config_schema,
            capabilities: PluginCapabilities::from_bits_retain(raw.capabilities),
            create_fn: raw.create_fn,
            destroy_fn: raw.destroy_fn,
        })
    }
}

/// Descriptor parsing errors.
#[derive(Debug, thiserror::Error)]
pub enum DescriptorError {
    #[error("ABI version mismatch: expected {expected}, found {found}")]
    AbiMismatch { expected: u32, found: u32 },

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid UTF-8 in field '{0}': {1}")]
    InvalidUtf8(String, #[source] std::string::FromUtf8Error),

    #[error("Invalid config schema JSON: {0}")]
    InvalidSchema(#[source] serde_json::Error),

    #[error("Capability not allowed: {0}")]
    CapabilityNotAllowed(String),

    #[error("Version requirement not satisfied: expected {expected}, found {found}")]
    VersionRequirement { expected: String, found: String },
}

/// Function type for creating a plugin instance.
/// Takes a JSON config string and its length, returns a pointer to the instance.
pub type PluginCreateFn = unsafe extern "C" fn(config_json: *const u8, config_len: usize) -> *mut ();

/// Function type for destroying a plugin instance.
/// Takes the instance pointer and frees all resources.
pub type PluginDestroyFn = unsafe extern "C" fn(instance: *mut ());

/// Display formatter for ParsedPluginDescriptor.
impl Display for ParsedPluginDescriptor {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} v{} ({}) - {}",
            self.name, self.version, self.id, self.plugin_type
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_default() {
        let caps = PluginCapabilities::default();
        assert!(caps.contains(PluginCapabilities::ASYNC));
        assert!(caps.contains(PluginCapabilities::THREAD_SAFE));
    }

    #[test]
    fn test_capabilities_flags() {
        let caps = PluginCapabilities::LIFECYCLE | PluginCapabilities::STREAMING;
        assert!(caps.contains(PluginCapabilities::LIFECYCLE));
        assert!(caps.contains(PluginCapabilities::STREAMING));
        assert!(!caps.contains(PluginCapabilities::NETWORK));
    }
}
