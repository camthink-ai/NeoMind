//! Extension loaders for native, WASM, and isolated extensions.

pub mod isolated;
pub mod native;
pub mod wasm;

pub use isolated::{IsolatedExtensionLoader, IsolatedLoaderConfig, LoadedExtension};
pub use native::{LoadedNativeExtension, NativeExtensionLoader};
pub use wasm::WasmExtensionLoader;
