//! Server state modules.
//!
//! This module organizes server state into logical sub-states:
//! - [`CoreState`]: Fundamental cross-cutting services
//! - [`DeviceState`]: Device management and telemetry
//! - [`AutomationState`]: Rules, automations, and transforms
//! - [`AgentState`]: AI agents and sessions
//! - [`AuthState`]: Authentication and authorization

mod auth_state;
mod agent_state;
mod automation_state;
mod core_state;
mod device_state;

pub use auth_state::AuthState;
pub use agent_state::{AgentState, AgentManager};
pub use automation_state::AutomationState;
pub use core_state::CoreState;
pub use device_state::{DeviceState, DeviceStatusUpdate};
