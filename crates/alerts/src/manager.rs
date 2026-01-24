//! Alert manager for creating, tracking, and sending alerts.
//!
//! The alert manager provides a centralized way to manage alerts
//! and route them through notification channels.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use edge_ai_core::EventBus;

use super::alert::{Alert, AlertId, AlertSeverity, AlertStatus};
use super::plugin::{ChannelPluginRegistry, DynAlertChannel};
use super::error::{Error, Result};

/// Type alias for optional EventBus
type OptionEventBus = Arc<RwLock<Option<Arc<EventBus>>>>;

/// Alert manager for handling alerts.
pub struct AlertManager {
    /// All alerts by ID
    alerts: Arc<RwLock<HashMap<AlertId, Alert>>>,
    /// Active alerts
    active_alerts: Arc<RwLock<HashMap<AlertId, Alert>>>,
    /// Notification channels
    channels: Arc<RwLock<ChannelPluginRegistry>>,
    /// Alert history
    history: Arc<RwLock<Vec<Alert>>>,
    /// Maximum history size
    max_history_size: usize,
    /// Alert rules for automatic alert generation
    rules: Arc<RwLock<Vec<AlertRule>>>,
    /// Optional event bus for publishing alert events
    event_bus: OptionEventBus,
}

impl AlertManager {
    /// Create a new alert manager.
    pub fn new() -> Self {
        Self {
            alerts: Arc::new(RwLock::new(HashMap::new())),
            active_alerts: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(ChannelPluginRegistry::new())),
            history: Arc::new(RwLock::new(Vec::new())),
            max_history_size: 10000,
            rules: Arc::new(RwLock::new(Vec::new())),
            event_bus: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new alert manager with a custom channel registry.
    pub fn with_registry(channels: Arc<RwLock<ChannelPluginRegistry>>) -> Self {
        Self {
            alerts: Arc::new(RwLock::new(HashMap::new())),
            active_alerts: Arc::new(RwLock::new(HashMap::new())),
            channels,
            history: Arc::new(RwLock::new(Vec::new())),
            max_history_size: 10000,
            rules: Arc::new(RwLock::new(Vec::new())),
            event_bus: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the maximum history size.
    pub fn with_max_history_size(mut self, size: usize) -> Self {
        self.max_history_size = size;
        self
    }

    /// Set the event bus for publishing alert events.
    pub async fn set_event_bus(&self, event_bus: Arc<EventBus>) {
        *self.event_bus.write().await = Some(event_bus);
    }

    /// Get a reference to the event bus (if set).
    pub async fn get_event_bus(&self) -> Option<Arc<EventBus>> {
        self.event_bus.read().await.clone()
    }

    /// Create and send a new alert.
    pub async fn create_alert(&self, alert: Alert) -> Result<Alert> {
        let id = alert.id.clone();
        let is_active = alert.is_active();
        let severity = alert.severity;

        // Store the alert
        self.alerts.write().await.insert(id.clone(), alert.clone());

        if is_active {
            self.active_alerts
                .write()
                .await
                .insert(id.clone(), alert.clone());
        }

        // Add to history
        self.add_to_history(alert.clone()).await;

        // Send through channels
        let channels = self.channels.read().await;
        channels.send_all(&alert).await;

        // Publish AlertCreated event to EventBus if configured
        if let Some(event_bus) = self.event_bus.read().await.as_ref() {
            use edge_ai_core::NeoTalkEvent;
            let severity_str = format!("{:?}", severity);
            let _ = event_bus.publish(NeoTalkEvent::AlertCreated {
                alert_id: id.to_string(),
                title: alert.title.clone(),
                severity: severity_str,
                message: alert.message.clone(),
                timestamp: alert.timestamp.timestamp(),
            });
            tracing::debug!("Published AlertCreated event for alert {}", id);
        }

        Ok(alert)
    }

    /// Create a simple alert with the given parameters.
    pub async fn alert(
        &self,
        severity: AlertSeverity,
        title: String,
        message: String,
        source: String,
    ) -> Result<Alert> {
        let alert = Alert::new(severity, title, message, source);
        self.create_alert(alert).await
    }

    /// Create a device alert.
    pub async fn device_alert(
        &self,
        severity: AlertSeverity,
        title: String,
        message: String,
        device_id: String,
    ) -> Result<Alert> {
        let alert = Alert::device(severity, title, message, device_id);
        self.create_alert(alert).await
    }

    /// Create a rule alert.
    pub async fn rule_alert(
        &self,
        severity: AlertSeverity,
        title: String,
        message: String,
        rule_id: String,
    ) -> Result<Alert> {
        let alert = Alert::rule(severity, title, message, rule_id);
        self.create_alert(alert).await
    }

    /// Get an alert by ID.
    pub async fn get_alert(&self, id: &AlertId) -> Option<Alert> {
        self.alerts.read().await.get(id).cloned()
    }

    /// List all alerts.
    pub async fn list_alerts(&self) -> Vec<Alert> {
        self.alerts.read().await.values().cloned().collect()
    }

    /// List active alerts.
    pub async fn list_active(&self) -> Vec<Alert> {
        self.active_alerts.read().await.values().cloned().collect()
    }

    /// List alerts by severity.
    pub async fn list_by_severity(&self, severity: AlertSeverity) -> Vec<Alert> {
        self.alerts
            .read()
            .await
            .values()
            .filter(|a| a.severity == severity)
            .cloned()
            .collect()
    }

    /// List alerts by source.
    pub async fn list_by_source(&self, source: &str) -> Vec<Alert> {
        self.alerts
            .read()
            .await
            .values()
            .filter(|a| a.source == source)
            .cloned()
            .collect()
    }

    /// List alerts by status.
    pub async fn list_by_status(&self, status: AlertStatus) -> Vec<Alert> {
        self.alerts
            .read()
            .await
            .values()
            .filter(|a| a.status == status)
            .cloned()
            .collect()
    }

    /// Acknowledge an alert.
    pub async fn acknowledge(&self, id: &AlertId) -> Result<()> {
        let mut alerts = self.alerts.write().await;
        if let Some(alert) = alerts.get_mut(id) {
            alert.acknowledge();
            self.active_alerts.write().await.remove(id);
            Ok(())
        } else {
            Err(Error::NotFound(format!("Alert not found: {}", id)))
        }
    }

    /// Resolve an alert.
    pub async fn resolve(&self, id: &AlertId) -> Result<()> {
        let mut alerts = self.alerts.write().await;
        if let Some(alert) = alerts.get_mut(id) {
            alert.resolve();
            self.active_alerts.write().await.remove(id);
            Ok(())
        } else {
            Err(Error::NotFound(format!("Alert not found: {}", id)))
        }
    }

    /// Mark an alert as false positive.
    pub async fn mark_false_positive(&self, id: &AlertId) -> Result<()> {
        let mut alerts = self.alerts.write().await;
        if let Some(alert) = alerts.get_mut(id) {
            alert.mark_false_positive();
            self.active_alerts.write().await.remove(id);
            Ok(())
        } else {
            Err(Error::NotFound(format!("Alert not found: {}", id)))
        }
    }

    /// Delete an alert.
    pub async fn delete_alert(&self, id: &AlertId) -> Result<()> {
        self.alerts.write().await.remove(id);
        self.active_alerts.write().await.remove(id);
        Ok(())
    }

    /// Get alert history.
    pub async fn get_history(&self) -> Vec<Alert> {
        self.history.read().await.clone()
    }

    /// Clear alert history.
    pub async fn clear_history(&self) {
        self.history.write().await.clear();
    }

    /// Add a notification channel.
    pub async fn add_channel(&self, channel: DynAlertChannel) {
        let channels = self.channels.read().await;
        let name = channel.name().to_string();
        channels.register_channel(name, channel).await;
    }

    /// Remove a notification channel.
    pub async fn remove_channel(&self, name: &str) -> bool {
        let channels = self.channels.read().await;
        channels.unregister_channel(name).await
    }

    /// List notification channels.
    pub async fn list_channels(&self) -> Vec<String> {
        let channels = self.channels.read().await;
        channels.list_channels().await
    }

    /// Get channel statistics.
    pub async fn get_channel_stats(&self) -> super::plugin::ChannelStats {
        let channels = self.channels.read().await;
        channels.get_stats().await
    }

    /// Get alert statistics.
    pub async fn get_stats(&self) -> AlertStats {
        let alerts = self.alerts.read().await;

        let total = alerts.len();
        let active = self.active_alerts.read().await.len();

        let mut by_severity: HashMap<String, usize> = HashMap::new();
        let mut by_status: HashMap<String, usize> = HashMap::new();
        let mut by_source: HashMap<String, usize> = HashMap::new();

        for alert in alerts.values() {
            *by_severity
                .entry(format!("{}", alert.severity))
                .or_insert(0) += 1;
            *by_status.entry(format!("{}", alert.status)).or_insert(0) += 1;
            *by_source.entry(alert.source.clone()).or_insert(0) += 1;
        }

        AlertStats {
            total,
            active,
            inactive: total - active,
            by_severity,
            by_status,
            by_source,
        }
    }

    /// Add an alert rule.
    pub async fn add_rule(&self, rule: AlertRule) {
        self.rules.write().await.push(rule);
    }

    /// Evaluate all alert rules.
    pub async fn evaluate_rules(&self) -> Vec<Alert> {
        let rules = self.rules.read().await;
        let mut triggered = Vec::new();

        for rule in rules.iter() {
            if let Some(alert) = rule.evaluate().await
                && let Ok(created) = self.create_alert(alert).await {
                    triggered.push(created);
                }
        }

        triggered
    }

    /// Add an alert to history.
    async fn add_to_history(&self, alert: Alert) {
        let mut history = self.history.write().await;
        history.push(alert);
        if history.len() > self.max_history_size {
            history.remove(0);
        }
    }

    /// Get the internal channel registry for advanced operations.
    pub async fn channel_registry(&self) -> Arc<RwLock<ChannelPluginRegistry>> {
        self.channels.clone()
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Alert statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlertStats {
    /// Total number of alerts
    pub total: usize,
    /// Number of active alerts
    pub active: usize,
    /// Number of inactive alerts
    pub inactive: usize,
    /// Alerts by severity
    pub by_severity: HashMap<String, usize>,
    /// Alerts by status
    pub by_status: HashMap<String, usize>,
    /// Alerts by source
    pub by_source: HashMap<String, usize>,
}

/// Rule for automatic alert generation.
pub enum AlertRule {
    /// Always triggers (for testing)
    AlwaysTrue(AlwaysTrueRule),
    /// Never triggers (for testing)
    AlwaysFalse(AlwaysFalseRule),
    /// Custom rule with a predicate function
    Custom(CustomRule),
}

impl AlertRule {
    /// Evaluate the rule and return an alert if triggered.
    pub async fn evaluate(&self) -> Option<Alert> {
        match self {
            Self::AlwaysTrue(rule) => rule.evaluate().await,
            Self::AlwaysFalse(rule) => rule.evaluate().await,
            Self::Custom(rule) => rule.evaluate().await,
        }
    }
}

/// Always-true rule for testing.
pub struct AlwaysTrueRule {
    pub severity: AlertSeverity,
    pub title_template: String,
    pub message_template: String,
}

impl AlwaysTrueRule {
    pub fn new(severity: AlertSeverity, title_template: String, message_template: String) -> Self {
        Self {
            severity,
            title_template,
            message_template,
        }
    }

    pub async fn evaluate(&self) -> Option<Alert> {
        Some(Alert::new(
            self.severity,
            self.title_template.clone(),
            self.message_template.clone(),
            "rule".to_string(),
        ))
    }
}

/// Always-false rule for testing.
pub struct AlwaysFalseRule;

impl Default for AlwaysFalseRule {
    fn default() -> Self {
        Self::new()
    }
}

impl AlwaysFalseRule {
    pub fn new() -> Self {
        Self
    }

    pub async fn evaluate(&self) -> Option<Alert> {
        None
    }
}

/// Custom rule with a predicate function.
pub struct CustomRule {
    pub severity: AlertSeverity,
    pub title_template: String,
    pub message_template: String,
    pub predicate: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl CustomRule {
    pub fn new(
        severity: AlertSeverity,
        title_template: String,
        message_template: String,
        predicate: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        Self {
            severity,
            title_template,
            message_template,
            predicate,
        }
    }

    pub async fn evaluate(&self) -> Option<Alert> {
        if (self.predicate)() {
            Some(Alert::new(
                self.severity,
                self.title_template.clone(),
                self.message_template.clone(),
                "rule".to_string(),
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::channels::ConsoleChannel;
    use super::*;

    #[tokio::test]
    async fn test_alert_manager_creation() {
        let manager = AlertManager::new();

        let alert = manager
            .alert(
                AlertSeverity::Warning,
                "Test Alert".to_string(),
                "Test message".to_string(),
                "test_source".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(alert.title, "Test Alert");
        assert!(manager.get_alert(&alert.id).await.is_some());
    }

    #[tokio::test]
    async fn test_device_alert() {
        let manager = AlertManager::new();

        let alert = manager
            .device_alert(
                AlertSeverity::Critical,
                "Device Offline".to_string(),
                "Sensor is offline".to_string(),
                "sensor_1".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(alert.source_type, "device");
        assert_eq!(alert.source, "sensor_1");
    }

    #[tokio::test]
    async fn test_alert_resolution() {
        let manager = AlertManager::new();

        let alert = manager
            .alert(
                AlertSeverity::Info,
                "Test".to_string(),
                "Test".to_string(),
                "test".to_string(),
            )
            .await
            .unwrap();

        assert!(manager.list_active().await.len() == 1);

        manager.resolve(&alert.id).await.unwrap();

        assert!(manager.list_active().await.len() == 0);
        let resolved = manager.get_alert(&alert.id).await.unwrap();
        assert_eq!(resolved.status, AlertStatus::Resolved);
    }

    #[tokio::test]
    async fn test_alert_statistics() {
        let manager = AlertManager::new();

        manager
            .alert(
                AlertSeverity::Warning,
                "Alert 1".to_string(),
                "Message 1".to_string(),
                "source1".to_string(),
            )
            .await
            .unwrap();

        manager
            .alert(
                AlertSeverity::Critical,
                "Alert 2".to_string(),
                "Message 2".to_string(),
                "source2".to_string(),
            )
            .await
            .unwrap();

        let stats = manager.get_stats().await;
        assert_eq!(stats.total, 2);
        assert_eq!(stats.active, 2);
    }

    #[tokio::test]
    async fn test_channel_integration() {
        let manager = AlertManager::new();

        let channel = Arc::new(ConsoleChannel::new("test_channel".to_string()));
        manager.add_channel(channel).await;

        let alert = manager
            .alert(
                AlertSeverity::Info,
                "Test".to_string(),
                "Test".to_string(),
                "test".to_string(),
            )
            .await
            .unwrap();

        // Alert should be in the channel
        let channels = manager.list_channels().await;
        assert!(channels.contains(&"test_channel".to_string()));
    }

    #[tokio::test]
    async fn test_alert_rules() {
        let manager = AlertManager::new();

        let rule = AlertRule::AlwaysTrue(AlwaysTrueRule::new(
            AlertSeverity::Warning,
            "Rule Triggered".to_string(),
            "The rule was triggered".to_string(),
        ));

        manager.add_rule(rule).await;

        let triggered = manager.evaluate_rules().await;
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].title, "Rule Triggered");
    }

    #[tokio::test]
    async fn test_history() {
        let manager = AlertManager::new();

        manager
            .alert(
                AlertSeverity::Info,
                "Alert 1".to_string(),
                "Message 1".to_string(),
                "test".to_string(),
            )
            .await
            .unwrap();

        manager
            .alert(
                AlertSeverity::Warning,
                "Alert 2".to_string(),
                "Message 2".to_string(),
                "test".to_string(),
            )
            .await
            .unwrap();

        let history = manager.get_history().await;
        assert_eq!(history.len(), 2);
    }

    #[tokio::test]
    async fn test_list_by_severity() {
        let manager = AlertManager::new();

        manager
            .alert(
                AlertSeverity::Warning,
                "Warning".to_string(),
                "Msg".to_string(),
                "test".to_string(),
            )
            .await
            .unwrap();

        manager
            .alert(
                AlertSeverity::Critical,
                "Critical".to_string(),
                "Msg".to_string(),
                "test".to_string(),
            )
            .await
            .unwrap();

        let warnings = manager.list_by_severity(AlertSeverity::Warning).await;
        assert_eq!(warnings.len(), 1);

        let criticals = manager.list_by_severity(AlertSeverity::Critical).await;
        assert_eq!(criticals.len(), 1);
    }

    #[tokio::test]
    async fn test_custom_rule() {
        let manager = AlertManager::new();

        // Create a rule that triggers when a counter reaches 3
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let rule = AlertRule::Custom(CustomRule::new(
            AlertSeverity::Warning,
            "Counter reached 3".to_string(),
            "The counter has reached 3".to_string(),
            Arc::new(move || {
                counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 >= 3
            }),
        ));

        manager.add_rule(rule).await;

        // First evaluation - counter becomes 1, no trigger
        let triggered = manager.evaluate_rules().await;
        assert_eq!(triggered.len(), 0);

        // Second evaluation - counter becomes 2, no trigger
        let triggered = manager.evaluate_rules().await;
        assert_eq!(triggered.len(), 0);

        // Third evaluation - counter becomes 3, triggers!
        let triggered = manager.evaluate_rules().await;
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].title, "Counter reached 3");
    }
}
