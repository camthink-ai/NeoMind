//! Extension loaders for native and isolated extensions.
//!
//! Extensions are native cdylib binaries discovered via metadata and
//! executed through the isolated extension-runner process.

pub mod isolated;
pub mod native;

pub use isolated::{IsolatedExtensionLoader, IsolatedLoaderConfig, LoadedExtension};
pub use native::NativeExtensionMetadataLoader;
