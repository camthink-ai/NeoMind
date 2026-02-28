//! Extension loaders for native and isolated extensions.
//!
//! WASM extensions are now handled by the extension-runner process,
//! which uses wasmtime directly for execution.

pub mod isolated;
pub mod native;

pub use isolated::{IsolatedExtensionLoader, IsolatedLoaderConfig, LoadedExtension};
pub use native::{LoadedNativeExtension, NativeExtensionLoader};
