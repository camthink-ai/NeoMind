//! Extension system for NeoMind.
//!
//! Extensions are dynamically loaded modules (.so/.dylib/.dll/.wasm) that extend
//! NeoMind's capabilities. They are distinct from user configurations like
//! LLM backends, device connections, or alert channels.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                  ExtensionRegistry                   │
//! │  - Manages extension lifecycle                       │
//! │  - Provides health monitoring                        │
//! │  - Handles discovery and loading                     │
//! └─────────────────────────────────────────────────────┘
//!                          │
//!          ┌───────────────┼───────────────┐
//!          ▼               ▼               ▼
//!   ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
//!   │ Native Ext  │ │  WASM Ext   │ │ Future Ext  │
//!   │ (.so/.dll)  │ │  (.wasm)    │ │  Types      │
//!   └─────────────┘ └─────────────┘ └─────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use neomind_core::extension::{ExtensionRegistry, ExtensionType};
//!
//! let registry = ExtensionRegistry::new();
//!
//! // Load extension from file
//! let meta = registry.load_from_path(&path).await?;
//!
//! // Start extension
//! registry.start(&meta.id).await?;
//! ```

pub mod loader;
pub mod registry;
pub mod types;

pub use loader::{NativeExtensionLoader, WasmExtensionLoader};
pub use registry::{ExtensionInfo, ExtensionRegistry};
pub use types::{
    DynExtension, Extension, ExtensionError, ExtensionMetadata, ExtensionState, ExtensionStats,
    ExtensionType, Result,
};
