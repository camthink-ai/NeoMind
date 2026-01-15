//! Security validation for dynamic plugins.
//!
//! This module provides security checks for loading dynamic plugins,
//! including path validation, signature verification, and capability checks.

use std::path::{Path, PathBuf};

use super::{DescriptorError, PluginDescriptor, PluginCapabilities};
use crate::plugin::PluginError;

/// Security context for plugin loading.
#[derive(Debug, Clone)]
pub struct SecurityContext {
    /// Allowed search paths for plugins
    pub allowed_paths: Vec<PathBuf>,

    /// Whether to verify plugin signatures
    pub verify_signatures: bool,

    /// Whether to allow network access from plugins
    pub allow_network: bool,

    /// Whether to allow file system access from plugins
    pub allow_file_system: bool,

    /// Maximum plugin file size (in bytes)
    pub max_file_size: usize,
}

impl Default for SecurityContext {
    fn default() -> Self {
        let mut ctx = Self {
            allowed_paths: Vec::new(),
            verify_signatures: false,
            allow_network: true,
            allow_file_system: true,
            max_file_size: 100 * 1024 * 1024, // 100 MB default
        };

        // Add default plugin directories
        if let Ok(user_plugin_dir) = std::env::var("NEOTALK_PLUGIN_DIR") {
            ctx.allowed_paths.push(PathBuf::from(user_plugin_dir));
        } else {
            // Default to ~/.neotalk/plugins
            if let Some(home) = dirs::home_dir() {
                ctx.allowed_paths.push(home.join(".neotalk").join("plugins"));
            }
        }

        // Add system plugin directory
        #[cfg(unix)]
        ctx.allowed_paths.push(PathBuf::from("/var/lib/neotalk/plugins"));

        #[cfg(windows)]
        if let Some(program_data) = std::env::var("ProgramData").ok() {
            ctx.allowed_paths.push(PathBuf::from(program_data).join("NeoTalk").join("plugins"));
        }

        ctx
    }
}

impl SecurityContext {
    /// Create a new security context with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an allowed search path.
    pub fn add_allowed_path(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.allowed_paths.push(path.as_ref().to_path_buf());
        self
    }

    /// Set whether to verify plugin signatures.
    pub fn with_signature_verification(mut self, verify: bool) -> Self {
        self.verify_signatures = verify;
        self
    }

    /// Set whether to allow network access.
    pub fn with_network_access(mut self, allow: bool) -> Self {
        self.allow_network = allow;
        self
    }

    /// Set whether to allow file system access.
    pub fn with_file_system_access(mut self, allow: bool) -> Self {
        self.allow_file_system = allow;
        self
    }

    /// Set maximum plugin file size.
    pub fn with_max_file_size(mut self, size: usize) -> Self {
        self.max_file_size = size;
        self
    }

    /// Validate a plugin file path.
    pub fn validate_path(&self, path: &Path) -> Result<(), PluginError> {
        // Check if path exists
        if !path.exists() {
            return Err(PluginError::NotFound(path.display().to_string()));
        }

        // Check if it's a file
        if !path.is_file() {
            return Err(PluginError::InvalidPlugin(format!(
                "Path is not a file: {}",
                path.display()
            )));
        }

        // Check file extension based on platform
        let valid_extensions = if cfg!(target_os = "macos") {
            ["dylib"]
        } else if cfg!(target_os = "linux") {
            ["so"]
        } else if cfg!(target_os = "windows") {
            ["dll"]
        } else {
            return Err(PluginError::UnsupportedPlatform);
        };

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| PluginError::InvalidPlugin("No file extension".into()))?;

        if !valid_extensions.contains(&ext) {
            return Err(PluginError::InvalidPlugin(format!(
                "Invalid extension: {}, expected one of: {:?}",
                ext, valid_extensions
            )));
        }

        // Check file size
        let metadata = std::fs::metadata(path).map_err(|e| {
            PluginError::InvalidPlugin(format!("Cannot read file metadata: {}", e))
        })?;

        let file_size = metadata.len() as usize;
        if file_size > self.max_file_size {
            return Err(PluginError::InvalidPlugin(format!(
                "File too large: {} bytes (max: {})",
                file_size, self.max_file_size
            )));
        }

        // Check if path is within allowed directories
        let canonical_path = path
            .canonicalize()
            .map_err(|e| PluginError::InvalidPlugin(format!("Cannot canonicalize: {}", e)))?;

        if !self.allowed_paths.is_empty() {
            let is_allowed = self.allowed_paths.iter().any(|allowed| {
                allowed
                    .canonicalize()
                    .ok()
                    .map(|canonical| canonical_path.starts_with(canonical))
                    .unwrap_or(false)
            });

            if !is_allowed {
                return Err(PluginError::SecurityViolation(format!(
                    "Plugin path is outside allowed directories: {}",
                    path.display()
                )));
            }
        }

        Ok(())
    }

    /// Validate a plugin descriptor.
    pub fn validate_descriptor(&self, descriptor: &PluginDescriptor) -> Result<(), DescriptorError> {
        // Parse the descriptor to check its validity
        let parsed = unsafe { super::ParsedPluginDescriptor::from_raw(descriptor)? };

        // Check required version
        if let Ok(req) = semver::VersionReq::parse(&parsed.required_neotalk) {
            let current_version = semver::Version::parse(env!("CARGO_PKG_VERSION"))
                .unwrap_or_else(|_| semver::Version::new(1, 0, 0));

            if !req.matches(&current_version) {
                return Err(DescriptorError::VersionRequirement {
                    expected: parsed.required_neotalk,
                    found: env!("CARGO_PKG_VERSION").to_string(),
                });
            }
        }

        // Check capabilities against security settings
        let caps = parsed.capabilities;
        if caps.contains(PluginCapabilities::NETWORK) && !self.allow_network {
            return Err(DescriptorError::CapabilityNotAllowed("network".into()));
        }

        if caps.contains(PluginCapabilities::FILE_SYSTEM) && !self.allow_file_system {
            return Err(DescriptorError::CapabilityNotAllowed("file_system".into()));
        }

        Ok(())
    }

    /// Verify plugin signature.
    ///
    /// TODO: Implement actual signature verification.
    pub fn verify_signature(&self, _path: &Path) -> Result<(), PluginError> {
        if self.verify_signatures {
            // TODO: Implement signature verification
            // 1. Read signature file alongside plugin
            // 2. Verify cryptographic signature
            // 3. Check certificate chain
            tracing::warn!("Signature verification requested but not yet implemented");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_context_default() {
        let ctx = SecurityContext::default();
        assert!(!ctx.verify_signatures);
        assert!(ctx.allow_network);
        assert!(ctx.allow_file_system);
        assert_eq!(ctx.max_file_size, 100 * 1024 * 1024);
    }

    #[test]
    fn test_security_context_builder() {
        let ctx = SecurityContext::new()
            .with_signature_verification(true)
            .with_network_access(false)
            .with_file_system_access(false)
            .with_max_file_size(50 * 1024 * 1024);

        assert!(ctx.verify_signatures);
        assert!(!ctx.allow_network);
        assert!(!ctx.allow_file_system);
        assert_eq!(ctx.max_file_size, 50 * 1024 * 1024);
    }
}
