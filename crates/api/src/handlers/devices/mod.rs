//! Device management handlers.
//!
//! Provides REST API for device management with MDL support.

pub mod auto_onboard;
pub mod compat;
pub mod crud;
pub mod discovery;
pub mod mdl;
pub mod metrics;
pub mod models;
pub mod telemetry;
pub mod types;
pub mod webhook;

// Re-export all handlers for use in routing
pub use auto_onboard::*;
pub use crud::*;
pub use discovery::*;
pub use mdl::*;
pub use metrics::*;
pub use telemetry::*;
pub use types::*;
pub use webhook::*;
