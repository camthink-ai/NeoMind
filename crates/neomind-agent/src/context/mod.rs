//! Business context module for AI-native automation.
//!
//! This module provides dynamic business context injection into LLM prompts,
//! enabling the agent to understand system state and handle vague user queries.

mod business_context;
mod device_registry;
mod dynamic_tools;
mod health;
mod meta_tools;
mod resource_index;
mod resource_resolver;
mod state_provider;

#[cfg(test)]
mod mock_devices;

pub use business_context::{BusinessContext, ContextRelevance, ContextScope};
pub use device_registry::{DeviceAlias, DeviceCapability, DeviceLocation, DeviceRegistry};
pub use dynamic_tools::DynamicToolGenerator;
pub use health::{
    calculate_health, calculate_health_with_config, ContextHealth, HealthCheckConfig, HealthStatus,
};
pub use meta_tools::{MetaTool, MetaToolRegistry, SearchContext};
pub use resource_index::{
    AccessType, AlertChannelResourceData, Capability, CapabilityType, DeviceResourceData,
    DeviceTypeResourceData, Resource, ResourceData, ResourceDataHelper, ResourceId, ResourceIndex,
    ResourceIndexStats, SearchQuery, SearchResult,
};
pub use resource_resolver::{
    ActionType, IntentCategory, MatchType, ResolvedIntent, ResourceMatch, ResourceResolver,
    SuggestedAction,
};
pub use state_provider::{StateProvider, SystemResource, SystemSnapshot};

#[cfg(test)]
pub use mock_devices::{generate_large_scale_devices, generate_mock_devices, get_device_summary};

use std::sync::Arc;
use tokio::sync::RwLock;

// Type aliases to reduce complexity
pub type SharedDeviceRegistry = Arc<DeviceRegistry>;
pub type SharedStateProvider = Arc<RwLock<StateProvider>>;
pub type SharedResourceIndex = Arc<RwLock<ResourceIndex>>;
pub type SharedBusinessContext = Arc<RwLock<BusinessContext>>;

/// Business context manager - aggregates all context sources.
#[derive(Clone)]
pub struct ContextManager {
    /// Device registry with aliases and locations
    device_registry: Arc<DeviceRegistry>,
    /// System state provider
    state_provider: Arc<RwLock<StateProvider>>,
    /// Meta-tool registry for context discovery
    meta_tools: Arc<MetaToolRegistry>,
}

impl ContextManager {
    /// Create a new context manager.
    pub fn new() -> Self {
        Self {
            device_registry: Arc::new(DeviceRegistry::new()),
            state_provider: Arc::new(RwLock::new(StateProvider::new())),
            meta_tools: Arc::new(MetaToolRegistry::new()),
        }
    }

    /// Get device registry reference.
    pub fn device_registry(&self) -> Arc<DeviceRegistry> {
        Arc::clone(&self.device_registry)
    }

    /// Get state provider reference.
    pub fn state_provider(&self) -> Arc<RwLock<StateProvider>> {
        Arc::clone(&self.state_provider)
    }

    /// Get meta-tool registry reference.
    pub fn meta_tools(&self) -> Arc<MetaToolRegistry> {
        Arc::clone(&self.meta_tools)
    }

    /// Build business context for a user query.
    /// This analyzes the query and injects relevant business context.
    pub async fn build_context(&self, query: &str) -> BusinessContext {
        // Get current system state
        let state = {
            let provider = self.state_provider.read().await;
            provider.get_snapshot().await
        };

        // Extract relevant devices from query
        let relevant_devices = self.device_registry.resolve_from_query(query);

        // Determine context scope based on query
        let scope = Self::determine_scope(query, &relevant_devices);

        BusinessContext {
            query: query.to_string(),
            scope,
            devices: relevant_devices,
            system_state: state,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Determine context scope from query analysis.
    fn determine_scope(query: &str, devices: &[DeviceAlias]) -> ContextScope {
        let query_lower = query.to_lowercase();

        // Check for specific device mentions
        let has_device = devices.iter().any(|d| d.matched);
        let has_location = query_lower.contains("客厅")
            || query_lower.contains("卧室")
            || query_lower.contains("厨房")
            || query_lower.contains("卫生间")
            || query_lower.contains("living")
            || query_lower.contains("bedroom")
            || query_lower.contains("kitchen");

        // Check for data query
        let is_data_query = query_lower.contains("温度")
            || query_lower.contains("湿度")
            || query_lower.contains("多少")
            || query_lower.contains("temperature")
            || query_lower.contains("humidity")
            || query_lower.contains("状态");

        // Check for control intent
        let is_control = query_lower.contains("打开")
            || query_lower.contains("关闭")
            || query_lower.contains("控制")
            || query_lower.contains("调节")
            || query_lower.contains("open")
            || query_lower.contains("close")
            || query_lower.contains("turn on")
            || query_lower.contains("turn off");

        // Check for list/query intent
        let is_list = query_lower.contains("有哪些")
            || query_lower.contains("列出")
            || query_lower.contains("所有")
            || query_lower.contains("list")
            || query_lower.contains("show all");

        match (has_device, has_location, is_data_query, is_control, is_list) {
            // Specific device mentioned - focused context
            (true, _, _, _, _) => ContextScope::Focused,

            // Location mentioned - location context
            (_, true, _, _, _) => ContextScope::Location,

            // List query - full context
            (_, _, _, _, true) => ContextScope::Full,

            // Data query without device - standard context
            (_, _, true, false, false) => ContextScope::Standard,

            // Control without device - need context discovery
            (_, _, false, true, false) => ContextScope::Discovery,

            // Default - minimal context
            _ => ContextScope::Minimal,
        }
    }

    /// Format business context for LLM prompt.
    pub async fn format_for_prompt(&self, query: &str) -> String {
        let context = self.build_context(query).await;
        context.format_for_prompt()
    }
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_scope() {
        // Specific device - returns Standard because it's a data query
        let scope = ContextManager::determine_scope("sensor_1的温度", &[]);
        assert_eq!(scope, ContextScope::Standard);

        // Location based
        let scope = ContextManager::determine_scope("客厅的灯", &[]);
        assert_eq!(scope, ContextScope::Location);

        // List query
        let scope = ContextManager::determine_scope("列出所有设备", &[]);
        assert_eq!(scope, ContextScope::Full);

        // Data query without device - returns Standard (needs context discovery)
        let scope = ContextManager::determine_scope("温度是多少", &[]);
        assert_eq!(scope, ContextScope::Standard);

        // Control without device - returns Discovery (needs context discovery)
        let scope = ContextManager::determine_scope("打开灯", &[]);
        assert_eq!(scope, ContextScope::Discovery);
    }
}
