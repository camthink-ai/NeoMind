//!
//! Event Dispatcher for Extension Event Distribution
//!
//! This module handles distributing events to subscribed extensions.
//!
//! Simplified Architecture:
//! 1. Extensions declare their event subscriptions via `event_subscriptions()` method
//! 2. EventDispatcher automatically registers these subscriptions when extensions are loaded
//! 3. When events are published, EventDispatcher calls `handle_event()` on subscribed extensions
//! 4. For isolated extensions, events are pushed via IPC to the extension process

use parking_lot::RwLock;
use serde_json::Value;
use tracing::{debug, error, info, trace};

use super::system::DynExtension;

/// Event dispatcher for pushing events to extensions
pub struct EventDispatcher {
    /// Extension event subscriptions: extension_id -> event_types
    subscriptions: RwLock<std::collections::HashMap<String, Vec<String>>>,
    /// Registered in-process extensions: extension_id -> extension
    in_process_extensions: RwLock<std::collections::HashMap<String, DynExtension>>,
    /// Event push channels for isolated extensions: extension_id -> sender
    isolated_event_senders:
        RwLock<std::collections::HashMap<String, tokio::sync::mpsc::Sender<(String, Value)>>>,
}

impl Default for EventDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl EventDispatcher {
    /// Create a new event dispatcher
    pub fn new() -> Self {
        Self {
            subscriptions: RwLock::new(std::collections::HashMap::new()),
            in_process_extensions: RwLock::new(std::collections::HashMap::new()),
            isolated_event_senders: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Register an in-process extension and its event subscriptions
    ///
    /// This method is called when an in-process extension is loaded. It automatically
    /// registers the extension's event subscriptions declared via `event_subscriptions()`.
    pub async fn register_in_process_extension(
        &self,
        extension_id: String,
        extension: DynExtension,
    ) {
        // Get the extension's event subscriptions (async)
        let event_types: Vec<String> = {
            let ext_guard = extension.read().await;
            let subscriptions: &[&str] = ext_guard.event_subscriptions();
            subscriptions.iter().map(|s| (*s).to_string()).collect()
        };

        // Store the extension
        self.in_process_extensions
            .write()
            .insert(extension_id.clone(), extension.clone());

        // Store the subscriptions
        if !event_types.is_empty() {
            self.subscriptions
                .write()
                .insert(extension_id.clone(), event_types.clone());
            info!(
                extension_id = %extension_id,
                event_types = ?event_types,
                "Registered in-process extension for event dispatch"
            );
        } else {
            info!(
                extension_id = %extension_id,
                "Registered in-process extension (no event subscriptions)"
            );
        }
    }

    /// Register an isolated extension and its event subscriptions
    ///
    /// This method is called when an isolated extension is loaded. It automatically
    /// registers the extension's event subscriptions and sets up the event push channel.
    pub fn register_isolated_extension(
        &self,
        extension_id: String,
        event_types: Vec<String>,
        event_sender: tokio::sync::mpsc::Sender<(String, Value)>,
    ) {
        // Store the event push channel
        self.isolated_event_senders
            .write()
            .insert(extension_id.clone(), event_sender);

        // Store the subscriptions
        if !event_types.is_empty() {
            self.subscriptions
                .write()
                .insert(extension_id.clone(), event_types.clone());
            info!(
                extension_id = %extension_id,
                event_types = ?event_types,
                "Registered isolated extension for event dispatch"
            );
        } else {
            info!(
                extension_id = %extension_id,
                "Registered isolated extension (no event subscriptions)"
            );
        }
    }

    /// Unregister an extension
    pub fn unregister_extension(&self, extension_id: &str) {
        self.in_process_extensions.write().remove(extension_id);
        self.isolated_event_senders.write().remove(extension_id);
        self.subscriptions.write().remove(extension_id);
        debug!(extension_id = %extension_id, "Unregistered extension from event dispatch");
    }

    /// Dispatch an event to all subscribed extensions
    ///
    /// This method is called by ExtensionEventSubscriptionService when an event
    /// is published to the EventBus. It calls `handle_event()` on all extensions
    /// that have subscribed to this event type.
    ///
    /// # Subscription Matching
    ///
    /// Extensions can subscribe to events in several ways:
    /// - Exact match: `["DeviceMetric"]` matches only "DeviceMetric" events
    /// - Prefix match: `["Device"]` matches "DeviceMetric", "DeviceOnline", etc.
    /// - Wildcard: `["all"]` matches all events
    ///
    /// # Event Format
    ///
    /// Events are dispatched in the following format:
    /// ```json
    /// {
    ///   "event_type": "DeviceMetric",
    ///   "payload": { ... event data ... },
    ///   "timestamp": 1234567890
    /// }
    /// ```
    pub async fn dispatch_event(&self, event_type: &str, payload: Value) {
        // Clone necessary data to avoid holding locks across await points
        let subscriptions = self.subscriptions.read().clone();
        let isolated_event_senders = self.isolated_event_senders.read().clone();

        // Diagnostic: log every dispatch call with subscription state.
        // High-frequency (fires per event — bursts of 20+ Hz when device
        // metrics are polled), so debug rather than info to avoid log
        // flooding in production.
        let subscriber_count: usize = subscriptions
            .iter()
            .filter(|(_, ets)| ets.iter().any(|et| et == "all" || et == event_type))
            .count();
        debug!(
            event_type = %event_type,
            total_subscribers = subscriber_count,
            registered_extensions = subscriptions.len(),
            isolated_senders = isolated_event_senders.len(),
            "EventDispatcher: dispatch_event called"
        );

        // Find all extensions that have subscribed to this event type
        for (extension_id, event_types) in subscriptions.iter() {
            // Check if this extension should receive this event
            let should_receive = event_types.iter().any(|et| {
                // Wildcard: subscribe to all events
                if et == "all" {
                    return true;
                }
                // Exact match
                if et == event_type {
                    return true;
                }
                // Prefix match with separator (e.g., "Device" matches "Device::Metric")
                if event_type.starts_with(&format!("{}::", et)) {
                    return true;
                }
                // Prefix match without separator (e.g., "Device" matches "DeviceMetric")
                // This allows subscribing to a category prefix like "Device" to receive
                // all device-related events like "DeviceMetric", "DeviceOnline", etc.
                if event_type.len() > et.len() && event_type.starts_with(et) {
                    return true;
                }
                false
            });

            if should_receive {
                // IMPORTANT: Check isolated extensions FIRST
                // Isolated extensions have priority over in-process proxies
                // This ensures events go to the actual extension process, not the proxy
                if let Some(sender) = isolated_event_senders.get(extension_id) {
                    // Send event to isolated extension via channel
                    match sender.try_send((event_type.to_string(), payload.clone())) {
                        Ok(_) => {
                            debug!(
                                extension_id = %extension_id,
                                event_type = %event_type,
                                "EventDispatcher: delivered to isolated extension"
                            );
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                            // Channel full — subscriber is too slow. Drop the event
                            // for THIS subscriber + continue to the next one (was:
                            // send().await blocked ALL subscribers behind the slow one).
                            tracing::warn!(
                                extension_id = %extension_id,
                                event_type = %event_type,
                                "EventDispatcher: isolated extension channel full — \
                                 event dropped (subscriber too slow)"
                            );
                        }
                        Err(e) => {
                            error!(
                                extension_id = %extension_id,
                                error = %e,
                                "Failed to send event to isolated extension"
                            );
                        }
                    }
                    continue; // Skip in-process check for isolated extensions
                }

                // Try in-process extensions (only if no isolated sender found)
                let extension_opt = {
                    let in_process_extensions = self.in_process_extensions.read();
                    in_process_extensions.get(extension_id).cloned()
                };

                if let Some(extension) = extension_opt {
                    trace!(
                        extension_id = %extension_id,
                        event_type = %event_type,
                        "Dispatching event to in-process extension"
                    );

                    // Call the extension's handle_event method (async)
                    let ext_guard = extension.read().await;
                    if let Err(e) = ext_guard.handle_event(event_type, &payload) {
                        error!(
                            extension_id = %extension_id,
                            event_type = %event_type,
                            error = %e,
                            "Failed to handle event in extension"
                        );
                    }
                }
            }
        }
    }

    /// Dispatch an event to all subscribed extensions, excluding one specific extension.
    ///
    /// This is the same as `dispatch_event` but skips the specified extension ID.
    /// Used for feedback loop prevention — e.g., when dispatching ExtensionOutput events,
    /// the extension that produced the output should not receive its own event back.
    pub async fn dispatch_event_excluding(
        &self,
        event_type: &str,
        payload: Value,
        exclude_extension_id: &str,
    ) {
        // Clone necessary data to avoid holding locks across await points
        let subscriptions = self.subscriptions.read().clone();
        let isolated_event_senders = self.isolated_event_senders.read().clone();

        // Find all extensions that have subscribed to this event type
        for (extension_id, event_types) in subscriptions.iter() {
            // Skip the excluded extension (feedback loop prevention)
            if extension_id == exclude_extension_id {
                continue;
            }

            // Check if this extension should receive this event
            let should_receive = event_types.iter().any(|et| {
                // Wildcard: subscribe to all events
                if et == "all" {
                    return true;
                }
                // Exact match
                if et == event_type {
                    return true;
                }
                // Prefix match with separator (e.g., "Device" matches "Device::Metric")
                if event_type.starts_with(&format!("{}::", et)) {
                    return true;
                }
                // Prefix match without separator (e.g., "Device" matches "DeviceMetric")
                if event_type.len() > et.len() && event_type.starts_with(et) {
                    return true;
                }
                false
            });

            if should_receive {
                // IMPORTANT: Check isolated extensions FIRST
                // Isolated extensions have priority over in-process proxies
                // This ensures events go to the actual extension process, not the proxy
                if let Some(sender) = isolated_event_senders.get(extension_id) {
                    // Send event to isolated extension via channel
                    match sender.try_send((event_type.to_string(), payload.clone())) {
                        Ok(_) => {
                            debug!(
                                extension_id = %extension_id,
                                event_type = %event_type,
                                "EventDispatcher: delivered to isolated extension"
                            );
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                            // Channel full — subscriber is too slow. Drop the event
                            // for THIS subscriber + continue to the next one (was:
                            // send().await blocked ALL subscribers behind the slow one).
                            tracing::warn!(
                                extension_id = %extension_id,
                                event_type = %event_type,
                                "EventDispatcher: isolated extension channel full — \
                                 event dropped (subscriber too slow)"
                            );
                        }
                        Err(e) => {
                            error!(
                                extension_id = %extension_id,
                                error = %e,
                                "Failed to send event to isolated extension"
                            );
                        }
                    }
                    continue; // Skip in-process check for isolated extensions
                }

                // Try in-process extensions (only if no isolated sender found)
                let extension_opt = {
                    let in_process_extensions = self.in_process_extensions.read();
                    in_process_extensions.get(extension_id).cloned()
                };

                if let Some(extension) = extension_opt {
                    trace!(
                        extension_id = %extension_id,
                        event_type = %event_type,
                        "Dispatching event to in-process extension"
                    );

                    // Call the extension's handle_event method (async)
                    let ext_guard = extension.read().await;
                    if let Err(e) = ext_guard.handle_event(event_type, &payload) {
                        error!(
                            extension_id = %extension_id,
                            event_type = %event_type,
                            error = %e,
                            "Failed to handle event in extension"
                        );
                    }
                }
            }
        }
    }

    /// Get all event subscriptions
    ///
    /// Returns a map of extension_id -> event_types
    pub fn get_subscriptions(&self) -> std::collections::HashMap<String, Vec<String>> {
        self.subscriptions.read().clone()
    }
}
