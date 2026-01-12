//! Edge AI Scenarios Crate
//!
//! This crate provides scenario management for the NeoTalk platform.
//!
//! ## Features
//!
//! - **Scenario Management**: Create, read, update, and delete scenarios
//! - **Templates**: Predefined scenario templates for common use cases
//! - **LLM Integration**: Generate LLM-friendly prompts from scenarios
//! - **Indexing**: Query scenarios by name, tag, category, or environment
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_scenarios::{ScenarioManager, ScenarioTemplates};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let manager = ScenarioManager::new();
//!
//!     // Create from template
//!     let scenario = manager
//!         .create_from_template(
//!             ScenarioTemplates::datacenter_temperature(),
//!             Some("My Datacenter".to_string()),
//!         )
//!         .await?;
//!
//!     // Add devices and rules
//!     manager.add_device_to_scenario(&scenario.id, "temp_sensor_1".to_string()).await?;
//!     manager.add_rule_to_scenario(&scenario.id, "high_temp_alert".to_string()).await?;
//!
//!     // Generate LLM prompt
//!     let prompt = manager.get_llm_prompt(&scenario.id).await?;
//!     println!("{}", prompt);
//!
//!     Ok(())
//! }
//! ```

pub mod scenario;
pub mod manager;
pub mod error;

pub use scenario::{
    Scenario, ScenarioId, ScenarioCategory, Environment, ScenarioMetadata,
    ScenarioTemplate, ScenarioTemplates,
};
pub use manager::{ScenarioManager, ScenarioStats};
pub use error::{Error, Result};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
