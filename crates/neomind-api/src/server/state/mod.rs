//! Server state modules.
//!
//! This module organizes server state into logical sub-states:
//! - [`CoreState`]: Fundamental cross-cutting services
//! - [`DeviceState`]: Device management and telemetry
//! - [`ExtensionState`]: Extension management with independent storage
//! - [`AutomationState`]: Rules, automations, and transforms
//! - [`AgentState`]: AI agents and sessions
//! - [`AuthState`]: Authentication and authorization

mod agent_state;
mod auth_state;
mod automation_state;
mod core_state;
mod device_state;
mod extension_state;

pub use agent_state::{AgentState, CleanSlateHolder};
pub use auth_state::AuthState;
pub use automation_state::AutomationState;
pub use core_state::CoreState;
pub use device_state::{DeviceState, DeviceStatusUpdate};
pub use extension_state::{
    ExtensionMetricsStorage, ExtensionRegistryAdapter, ExtensionState, ExtensionStore,
};
