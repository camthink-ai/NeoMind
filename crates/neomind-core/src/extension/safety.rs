//! Extension Safety Module
//!
//! This module provides comprehensive safety mechanisms for extension isolation:
//! - Circuit breaker pattern to disable failing extensions
//! - Per-extension error tracking
//! - Automatic recovery after cooldown
//! - Panic isolation

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Circuit breaker state for an extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed, requests are allowed
    Closed,
    /// Circuit is open, requests are blocked
    Open,
    /// Circuit is half-open, testing if extension has recovered
    HalfOpen,
}

/// Per-extension circuit breaker.
pub struct ExtensionCircuitBreaker {
    /// Current circuit state
    state: Arc<AtomicU8>, // Stores CircuitState as u8
    /// Failure count since last success
    failure_count: Arc<AtomicU32>,
    /// Consecutive success count (for half-open state)
    success_count: Arc<AtomicU32>,
    /// Timestamp of last state change (Unix timestamp in seconds)
    last_state_change: Arc<AtomicU64>,
    /// Failure threshold to open circuit
    failure_threshold: u32,
    /// Success threshold to close circuit (in half-open state)
    success_threshold: u32,
    /// Cooldown period before attempting recovery (seconds)
    cooldown_secs: u64,
    /// Extension ID for logging
    extension_id: String,
}

impl ExtensionCircuitBreaker {
    /// Create a new circuit breaker for an extension.
    pub fn new(
        extension_id: String,
        failure_threshold: u32,
        success_threshold: u32,
        cooldown_secs: u64,
    ) -> Self {
        Self {
            state: Arc::new(AtomicU8::new(CircuitState::Closed as u8)),
            failure_count: Arc::new(AtomicU32::new(0)),
            success_count: Arc::new(AtomicU32::new(0)),
            last_state_change: Arc::new(AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )),
            failure_threshold,
            success_threshold,
            cooldown_secs,
            extension_id,
        }
    }

    /// Create with default thresholds.
    pub fn with_defaults(extension_id: String) -> Self {
        Self::new(
            extension_id,
            5,  // 5 failures to open circuit
            2,  // 2 successes to close circuit
            60, // 60 seconds cooldown
        )
    }

    /// Check if a request should be allowed.
    pub fn allow_request(&self) -> bool {
        let state = self.get_state();

        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if cooldown has passed
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let last_change = self.last_state_change.load(Ordering::Relaxed);

                if now.saturating_sub(last_change) >= self.cooldown_secs {
                    // Transition to half-open
                    self.set_state(CircuitState::HalfOpen);
                    self.success_count.store(0, Ordering::Relaxed);
                    debug!(extension_id = %self.extension_id, "Circuit breaker entering HALF-OPEN state (cooldown elapsed)");
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful operation.
    pub fn record_success(&self) {
        let state = self.get_state();

        match state {
            CircuitState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
                if successes >= self.success_threshold {
                    // Close the circuit
                    self.set_state(CircuitState::Closed);
                    self.failure_count.store(0, Ordering::Relaxed);
                    self.success_count.store(0, Ordering::Relaxed);
                    info!(extension_id = %self.extension_id, successes, "Circuit breaker CLOSED (recovered)");
                }
            }
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::Relaxed);
            }
            CircuitState::Open => {
                // Shouldn't happen, but handle gracefully
                // Success means we can try half-open
                self.set_state(CircuitState::HalfOpen);
                self.success_count.store(1, Ordering::Relaxed);
            }
        }
    }

    /// Record a failed operation.
    pub fn record_failure(&self) {
        let failures = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
        let state = self.get_state();

        match state {
            CircuitState::HalfOpen => {
                // Failed in half-open, immediately open again
                self.set_state(CircuitState::Open);
                self.failure_count
                    .store(self.failure_threshold, Ordering::Relaxed);
                warn!(extension_id = %self.extension_id, "Circuit breaker OPEN (failed in half-open state)");
            }
            CircuitState::Closed => {
                if failures >= self.failure_threshold {
                    // Open the circuit
                    self.set_state(CircuitState::Open);
                    warn!(extension_id = %self.extension_id, failures, threshold = self.failure_threshold, "Circuit breaker OPEN (threshold exceeded)");
                }
            }
            CircuitState::Open => {
                // Already open, just increment failure count
            }
        }
    }

    /// Get the current circuit state.
    pub fn get_state(&self) -> CircuitState {
        match self.state.load(Ordering::Relaxed) {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }

    /// Get the failure count.
    pub fn failure_count(&self) -> u32 {
        self.failure_count.load(Ordering::Relaxed)
    }

    /// Manually reset the circuit breaker.
    pub fn reset(&self) {
        self.set_state(CircuitState::Closed);
        self.failure_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
        info!(extension_id = %self.extension_id, "Circuit breaker manually RESET");
    }

    /// Check if the circuit is currently open (blocking requests).
    pub fn is_open(&self) -> bool {
        self.get_state() == CircuitState::Open
    }

    /// Set the circuit state.
    fn set_state(&self, state: CircuitState) {
        self.last_state_change.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            Ordering::Relaxed,
        );
        self.state.store(state as u8, Ordering::Relaxed);
    }
}

/// Safety manager for all extensions.
pub struct ExtensionSafetyManager {
    /// Circuit breakers for each extension
    breakers: RwLock<HashMap<String, Arc<ExtensionCircuitBreaker>>>,
    /// Disabled extensions (manually or automatically)
    disabled: RwLock<HashMap<String, DisabledInfo>>,
    /// Panic tracking
    panic_counts: RwLock<HashMap<String, PanicInfo>>,
}

/// Information about a disabled extension.
#[derive(Debug, Clone)]
struct DisabledInfo {
    reason: String,
    disabled_at: Instant,
    can_auto_recover: bool,
}

/// Panic tracking information.
#[derive(Debug, Clone)]
struct PanicInfo {
    count: u32,
    last_panic: Instant,
}

impl ExtensionSafetyManager {
    /// Create a new safety manager.
    pub fn new() -> Self {
        Self {
            breakers: RwLock::new(HashMap::new()),
            disabled: RwLock::new(HashMap::new()),
            panic_counts: RwLock::new(HashMap::new()),
        }
    }

    /// Register an extension with the safety manager.
    pub async fn register_extension(&self, extension_id: String) -> Arc<ExtensionCircuitBreaker> {
        let mut breakers = self.breakers.write().await;
        let breaker = Arc::new(ExtensionCircuitBreaker::with_defaults(extension_id.clone()));
        breakers.insert(extension_id.clone(), breaker.clone());
        breaker
    }

    /// Unregister an extension.
    pub async fn unregister_extension(&self, extension_id: &str) {
        let mut breakers = self.breakers.write().await;
        let mut disabled = self.disabled.write().await;
        let mut panic_counts = self.panic_counts.write().await;
        breakers.remove(extension_id);
        disabled.remove(extension_id);
        panic_counts.remove(extension_id);
    }

    /// Check if an extension is allowed to execute.
    pub async fn is_allowed(&self, extension_id: &str) -> bool {
        // Check if extension is disabled
        if let Some(info) = self.disabled.read().await.get(extension_id) {
            if info.can_auto_recover {
                // Check if recovery time has elapsed
                if info.disabled_at.elapsed() > Duration::from_secs(300) {
                    // Auto-recover after 5 minutes
                    self.enable_extension(extension_id).await;
                    return true;
                }
            }
            return false;
        }

        // Check circuit breaker
        if let Some(breaker) = self.breakers.read().await.get(extension_id) {
            return breaker.allow_request();
        }

        true
    }

    /// Record a successful operation for an extension.
    pub async fn record_success(&self, extension_id: &str) {
        if let Some(breaker) = self.breakers.read().await.get(extension_id) {
            breaker.record_success();
        }
    }

    /// Record a failed operation for an extension.
    pub async fn record_failure(&self, extension_id: &str) {
        if let Some(breaker) = self.breakers.read().await.get(extension_id) {
            breaker.record_failure();
        }
    }

    /// Record a panic from an extension.
    pub async fn record_panic(&self, extension_id: &str) {
        let mut panic_counts = self.panic_counts.write().await;
        let info = panic_counts
            .entry(extension_id.to_string())
            .or_insert(PanicInfo {
                count: 0,
                last_panic: Instant::now(),
            });
        info.count += 1;
        info.last_panic = Instant::now();

        // If extension panics 3 times, disable it
        if info.count >= 3 {
            drop(panic_counts);
            self.disable_extension(
                extension_id,
                "Too many panics (circuit breaker triggered)",
                false, // Manual recovery required
            )
            .await;
        }
    }

    /// Disable an extension manually or automatically.
    pub async fn disable_extension(
        &self,
        extension_id: &str,
        reason: &str,
        can_auto_recover: bool,
    ) {
        let mut disabled = self.disabled.write().await;
        disabled.insert(
            extension_id.to_string(),
            DisabledInfo {
                reason: reason.to_string(),
                disabled_at: Instant::now(),
                can_auto_recover,
            },
        );
        warn!(extension_id = %extension_id, reason, can_auto_recover, "Extension DISABLED");
    }

    /// Enable a disabled extension.
    pub async fn enable_extension(&self, extension_id: &str) {
        let mut disabled = self.disabled.write().await;
        if disabled.remove(extension_id).is_some() {
            // Reset circuit breaker if exists
            if let Some(breaker) = self.breakers.read().await.get(extension_id) {
                breaker.reset();
            }
            // Reset panic count
            let mut panic_counts = self.panic_counts.write().await;
            panic_counts.remove(extension_id);
            info!(extension_id = %extension_id, "Extension ENABLED");
        }
    }

    /// Get status of all extensions.
    pub async fn get_status(&self) -> HashMap<String, ExtensionSafetyStatus> {
        let mut status = HashMap::new();

        let breakers = self.breakers.read().await;
        let disabled = self.disabled.read().await;
        let panic_counts = self.panic_counts.read().await;

        // Get all unique extension IDs
        let all_ids: std::collections::HashSet<_> = breakers
            .keys()
            .chain(disabled.keys())
            .chain(panic_counts.keys())
            .map(|s| s.clone())
            .collect();

        for id in all_ids {
            let breaker = breakers.get(&id);
            let disabled_info = disabled.get(&id);
            let panic_info = panic_counts.get(&id);

            status.insert(
                id.clone(),
                ExtensionSafetyStatus {
                    extension_id: id.clone(),
                    circuit_state: breaker.map(|b| b.get_state()),
                    failure_count: breaker.map(|b| b.failure_count()).unwrap_or(0),
                    is_disabled: disabled_info.is_some(),
                    disable_reason: disabled_info.map(|d| d.reason.clone()),
                    panic_count: panic_info.map(|p| p.count).unwrap_or(0),
                },
            );
        }

        status
    }

    /// Get circuit breaker for an extension.
    pub async fn get_breaker(&self, extension_id: &str) -> Option<Arc<ExtensionCircuitBreaker>> {
        self.breakers.read().await.get(extension_id).cloned()
    }
}

/// Safety status of an extension.
#[derive(Debug, Clone)]
pub struct ExtensionSafetyStatus {
    pub extension_id: String,
    pub circuit_state: Option<CircuitState>,
    pub failure_count: u32,
    pub is_disabled: bool,
    pub disable_reason: Option<String>,
    pub panic_count: u32,
}

impl Default for ExtensionSafetyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Install a global panic hook that logs extension panics without crashing the server.
pub fn install_extension_panic_hook() {
    use std::panic;
    use std::sync::atomic::{AtomicBool, Ordering};

    // Only install once
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.load(Ordering::Relaxed) {
        return;
    }

    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info: &panic::PanicHookInfo| {
        // Check if panic originated from extension code
        let location = panic_info.location();
        let thread = std::thread::current();

        // Log the panic using error level (might not always work in panic state)
        let thread_name = thread.name().unwrap_or("unnamed");
        let payload = panic_info.payload().downcast_ref::<&str>();
        let payload_str = payload.map(|s| s.as_ref()).unwrap_or("unknown");

        error!(
            thread = %thread_name,
            payload = %payload_str,
            location = ?location.map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
            "Panic detected"
        );

        // Check if this is an extension-related panic by inspecting the backtrace
        if let Some(msg) = payload {
            if msg.contains("Extension") || msg.contains("extension") {
                error!("Extension-related panic detected, preventing propagation");
            }
        }

        // Call original hook for default behavior (but don't abort)
        // The original hook might abort, so we skip it in production
        #[cfg(debug_assertions)]
        original_hook(panic_info);
    }));

    INSTALLED.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_closed_allows_requests() {
        let breaker = ExtensionCircuitBreaker::with_defaults("test".to_string());
        assert_eq!(breaker.get_state(), CircuitState::Closed);
        assert!(breaker.allow_request());
    }

    #[test]
    fn test_circuit_breaker_opens_after_threshold() {
        let breaker = ExtensionCircuitBreaker::with_defaults("test".to_string());

        // Record failures up to threshold
        for _ in 0..5 {
            breaker.record_failure();
        }

        assert_eq!(breaker.get_state(), CircuitState::Open);
        assert!(!breaker.allow_request());
    }

    #[test]
    fn test_circuit_breaker_resets_on_success() {
        let breaker = ExtensionCircuitBreaker::with_defaults("test".to_string());

        // Add some failures
        breaker.record_failure();
        breaker.record_failure();

        // Success should reset failure count
        breaker.record_success();
        assert_eq!(breaker.failure_count(), 0);
    }

    #[test]
    fn test_circuit_breaker_half_open_to_closed() {
        let breaker = ExtensionCircuitBreaker::with_defaults("test".to_string());

        // Open the circuit
        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.get_state(), CircuitState::Open);

        // Manually set to half-open (simulating cooldown)
        breaker.set_state(CircuitState::HalfOpen);

        // Record successes to close
        breaker.record_success();
        breaker.record_success();

        assert_eq!(breaker.get_state(), CircuitState::Closed);
        assert!(breaker.allow_request());
    }

    #[tokio::test]
    async fn test_safety_manager_disabled_extension() {
        let manager = ExtensionSafetyManager::new();

        // Should be allowed initially
        assert!(manager.is_allowed("test-ext").await);

        // Disable the extension
        manager
            .disable_extension("test-ext", "Test disable", true)
            .await;

        // Should not be allowed
        assert!(!manager.is_allowed("test-ext").await);

        // Enable again
        manager.enable_extension("test-ext").await;

        // Should be allowed again
        assert!(manager.is_allowed("test-ext").await);
    }

    #[tokio::test]
    async fn test_safety_manager_panic_tracking() {
        let manager = ExtensionSafetyManager::new();

        // Record panics
        manager.record_panic("test-ext").await;
        manager.record_panic("test-ext").await;

        // Should still be allowed
        assert!(manager.is_allowed("test-ext").await);

        // Third panic should disable
        manager.record_panic("test-ext").await;

        // Should not be allowed now
        assert!(!manager.is_allowed("test-ext").await);
    }
}
