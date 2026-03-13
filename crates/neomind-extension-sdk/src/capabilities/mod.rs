//! Unified Capabilities Module (Native + WASM support)
//!
//! This module provides capability APIs that work seamlessly
//! on both Native and WASM targets through conditional compilation.
//!
//! # Platform Support Matrix
//!
//! | Capability | Native | WASM | Implementation |
//! |------------|--------|------|----------------|
//! | Device Metrics Read | ✅ async | ✅ sync | Unified API |
//! | Device Metrics Write | ✅ async | ✅ sync | Unified API |
//! | Device Control | ✅ async | ✅ sync | Unified API |
//! | Storage Query | ✅ async | ✅ sync | Unified API |
//! | Telemetry History | ✅ async | ✅ sync | Host proxy |
//! | Metrics Aggregate | ✅ async | ✅ sync | Host proxy |
//! | Event Publish | ✅ async | ✅ sync | Unified API |
//! | Event Subscribe | ✅ async | ✅ poll | Polling mode |
//! | Extension Call | ✅ async | ✅ sync | Host proxy |
//! | Agent Trigger | ✅ async | ✅ sync | Host proxy |
//! | Rule Trigger | ✅ async | ✅ sync | Host proxy |
//!
//! # Design Philosophy
//!
//! - **Single API**: Developers learn one API that works everywhere
//! - **Automatic routing**: Context.invoke_capability() routes to the right backend
//! - **Minimal FFI**: Only 1-2 host functions needed internally
//! - **Type safety**: Same type definitions across platforms
//!
//! # Usage
//!
//! ```rust,ignore
//! use neomind_extension_sdk::capabilities::device;
//! use neomind_extension_sdk::capabilities::storage;
//!
//! // Works on both Native and WASM
//! // Native: async execution
//! // WASM: synchronous execution (host handles async internally)
//! let metrics = device::get_metrics(&ctx, "device-1")?;
//! let temp = device::get_metric_typed::<f64>(&ctx, "device-1", "temperature")?;
//!
//! // Write virtual metrics
//! device::write_virtual_metric(&ctx, "device-1", "status", &json!("active"))?;
//!
//! // Query telemetry history (now works on WASM!)
//! let history = device::query_telemetry(&ctx, "device-1", "temp", start, end)?;
//!
//! // Query storage (new!)
//! let latest = storage::get_latest(&ctx, "device-1", "temperature")?;
//! let range = storage::get_range(&ctx, "device-1", "temperature", start, end)?;
//!
//! // Subscribe to events
//! let sub = event::subscribe(&ctx, "device_changed", None)?;
//! let events = sub.poll()?; // Polling mode on WASM
//! ```

pub mod agent;
pub mod device;
pub mod event;
pub mod extension;
pub mod rule;
pub mod storage;

// Re-export error type
pub use device::CapabilityError;

// Re-export storage types
pub use storage::{MetricValue, DeviceMetrics};

// Re-export core types (Native only)
#[cfg(not(target_arch = "wasm32"))]
pub use neomind_core::extension::context::*;

#[cfg(not(target_arch = "wasm32"))]
pub use neomind_core::event::NeoMindEvent;

// Re-export CapabilityContext for extensions
#[cfg(not(target_arch = "wasm32"))]
pub use neomind_core::extension::system::CapabilityContext;