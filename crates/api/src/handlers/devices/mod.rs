//! Device management handlers.
//!
//! Provides REST API for device management with MDL support.

pub mod models;
pub mod crud;
pub mod metrics;
pub mod types;
pub mod discovery;
pub mod hass;
pub mod mdl;
pub mod telemetry;

// Re-export all handlers for use in routing
pub use crud::*;
pub use metrics::*;
pub use types::*;
pub use discovery::*;
pub use hass::*;
pub use mdl::*;
pub use telemetry::*;
