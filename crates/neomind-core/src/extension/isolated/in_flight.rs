//! In-flight request tracking for concurrent IPC
//!
//! This module provides a high-performance mechanism for tracking concurrent
//! IPC requests and routing responses to the correct caller using oneshot channels.
//!
//! # Design
//!
//! Based on patterns from tarpc and other mature RPC frameworks:
//! - Each request gets a unique ID (atomic counter)
//! - A oneshot channel is created for each request
//! - The sender is stored in a HashMap keyed by request_id
//! - When a response arrives, it's routed to the correct oneshot channel
//! - Timeouts are handled via tokio::time::timeout

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::oneshot;
use tokio::time::{timeout, Duration};

use super::IpcResponse;

/// Unique identifier for a pending request
pub type RequestId = u64;

/// Sender for a pending request's response
type PendingResponse = oneshot::Sender<IpcResponse>;

/// Internal state for in-flight requests
struct InFlightState {
    /// Map of request_id -> oneshot sender
    pending: HashMap<RequestId, PendingResponse>,
    /// Next request ID (lock-free counter)
    next_id: AtomicU64,
}

/// Tracker for in-flight IPC requests
///
/// This structure manages the lifecycle of concurrent IPC requests,
/// allowing multiple requests to be in-flight simultaneously while
/// ensuring responses are correctly routed to their callers.
///
/// Uses `std::sync::Mutex` instead of tokio's async mutex so that
/// `complete()`, `cancel()`, etc. can be called from synchronous
/// contexts (e.g., the receiver thread) without `block_on`.
#[derive(Clone)]
pub struct InFlightRequests {
    /// Shared state protected by std::sync::Mutex (non-async)
    state: Arc<std::sync::Mutex<InFlightState>>,
    /// Default timeout for requests
    default_timeout: Duration,
}

impl InFlightRequests {
    /// Create a new in-flight request tracker
    pub fn new(default_timeout: Duration) -> Self {
        Self {
            state: Arc::new(std::sync::Mutex::new(InFlightState {
                pending: HashMap::new(),
                next_id: AtomicU64::new(1), // Start at 1, 0 is reserved for init
            })),
            default_timeout,
        }
    }

    /// Get the default timeout
    pub fn default_timeout(&self) -> Duration {
        self.default_timeout
    }

    /// Generate a new unique request ID
    pub fn next_request_id(&self) -> RequestId {
        self.state
            .lock()
            .map(|s| s.next_id.fetch_add(1, Ordering::Relaxed))
            .unwrap_or_else(|e| {
                // Fallback: use timestamp-based ID if lock is poisoned
                tracing::error!(error = %e, "InFlightRequests mutex poisoned, using fallback ID");
                match std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                {
                    Ok(duration) => duration.as_nanos() as u64,
                    Err(err) => {
                        tracing::error!(error = %err, "SystemTime error, using constant fallback ID");
                        0
                    }
                }
            })
    }

    /// Register a new pending request
    ///
    /// Returns the request ID and a oneshot receiver for the response.
    /// The caller should wait on the receiver with a timeout.
    pub fn register(&self) -> (RequestId, oneshot::Receiver<IpcResponse>) {
        let mut state = self.state.lock().unwrap_or_else(|e| {
            tracing::error!(error = %e, "InFlightRequests mutex poisoned in register, recovering");
            e.into_inner()
        });
        let request_id = state.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx): (oneshot::Sender<IpcResponse>, oneshot::Receiver<IpcResponse>) =
            oneshot::channel();

        state.pending.insert(request_id, tx);

        (request_id, rx)
    }

    /// Register a request with a specific ID (for initialization)
    pub fn register_with_id(&self, request_id: RequestId) -> oneshot::Receiver<IpcResponse> {
        let (tx, rx): (oneshot::Sender<IpcResponse>, oneshot::Receiver<IpcResponse>) =
            oneshot::channel();

        let mut state = self.state.lock().unwrap_or_else(|e| {
            tracing::error!(error = %e, "InFlightRequests mutex poisoned in register_with_id, recovering");
            e.into_inner()
        });
        state.pending.insert(request_id, tx);

        rx
    }

    /// Complete a pending request with a response
    ///
    /// Routes the response to the correct caller via the oneshot channel.
    /// Returns true if the request was found and completed, false otherwise.
    /// This is a sync method — safe to call from non-async contexts.
    pub fn complete(&self, request_id: RequestId, response: IpcResponse) -> bool {
        // Remove the sender from the map while holding the lock,
        // then send outside the critical section to reduce contention.
        let tx = {
            let mut state = match self.state.lock() {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(request_id, "InFlightRequests mutex poisoned: {}", e);
                    return false;
                }
            };
            state.pending.remove(&request_id)
        };

        if let Some(tx) = tx {
            // Send the response to the waiting caller (outside the lock)
            // Ignore send errors (caller may have timed out and dropped the receiver)
            let _: Result<_, _> = tx.send(response);
            true
        } else {
            tracing::debug!(
                request_id,
                "Received response for unknown request (may have timed out)"
            );
            false
        }
    }

    /// Cancel a pending request (e.g., on timeout)
    ///
    /// Removes the request from the pending map without sending a response.
    pub fn cancel(&self, request_id: RequestId) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.remove(&request_id);
        }
    }

    /// Get the number of pending requests
    pub fn pending_count(&self) -> usize {
        self.state.lock().map(|s| s.pending.len()).unwrap_or(0)
    }

    /// Cancel all pending requests
    ///
    /// Used during shutdown to clean up any waiting callers.
    pub fn cancel_all(&self) -> usize {
        if let Ok(mut state) = self.state.lock() {
            let count = state.pending.len();
            state.pending.clear();
            count
        } else {
            0
        }
    }

    /// Wait for a response with timeout
    ///
    /// Helper method that combines receiving with timeout handling.
    /// Automatically cancels the request on timeout.
    pub async fn wait_with_timeout(
        &self,
        request_id: RequestId,
        rx: oneshot::Receiver<IpcResponse>,
        timeout_duration: Duration,
    ) -> Result<IpcResponse, InFlightError> {
        match timeout(timeout_duration, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                // Channel closed (sender was dropped)
                self.cancel(request_id);
                Err(InFlightError::ChannelClosed)
            }
            Err(_) => {
                // Timeout
                self.cancel(request_id);
                Err(InFlightError::Timeout(timeout_duration.as_millis() as u64))
            }
        }
    }
}

impl Default for InFlightRequests {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

/// Errors that can occur while waiting for a response
#[derive(Debug, Clone, thiserror::Error)]
pub enum InFlightError {
    /// The request timed out
    #[error("Request timed out after {0}ms")]
    Timeout(u64),

    /// The response channel was closed
    #[error("Response channel closed")]
    ChannelClosed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_complete() {
        let tracker = InFlightRequests::new(Duration::from_secs(5));

        let (request_id, rx) = tracker.register();

        // Complete the request (sync call)
        let response = IpcResponse::Pong {
            request_id: 0,
            timestamp: 123,
        };
        let found = tracker.complete(request_id, response.clone());
        assert!(found);

        // Receive the response
        let received = rx.await.unwrap();
        assert!(matches!(
            received,
            IpcResponse::Pong {
                request_id: 0,
                timestamp: 123
            }
        ));
    }

    #[tokio::test]
    async fn test_timeout() {
        let tracker = InFlightRequests::new(Duration::from_secs(5));

        let (request_id, rx) = tracker.register();

        // Wait with a very short timeout
        let result = tracker
            .wait_with_timeout(request_id, rx, Duration::from_millis(10))
            .await;

        assert!(matches!(result, Err(InFlightError::Timeout(_))));
    }

    #[test]
    fn test_unknown_request() {
        let tracker = InFlightRequests::new(Duration::from_secs(5));

        // Try to complete a request that was never registered
        let response = IpcResponse::Pong {
            request_id: 0,
            timestamp: 123,
        };
        let found = tracker.complete(999, response);
        assert!(!found);
    }

    #[tokio::test]
    async fn test_concurrent_requests() {
        let tracker = InFlightRequests::new(Duration::from_secs(5));

        // Register multiple requests
        let (id1, rx1) = tracker.register();
        let (id2, rx2) = tracker.register();
        let (id3, rx3) = tracker.register();

        assert_eq!(tracker.pending_count(), 3);

        // Complete them in different order (sync calls)
        tracker.complete(
            id2,
            IpcResponse::Pong {
                request_id: 0,
                timestamp: 2,
            },
        );
        tracker.complete(
            id1,
            IpcResponse::Pong {
                request_id: 0,
                timestamp: 1,
            },
        );
        tracker.complete(
            id3,
            IpcResponse::Pong {
                request_id: 0,
                timestamp: 3,
            },
        );

        // Each receiver should get the correct response
        let r1 = rx1.await.unwrap();
        let r2 = rx2.await.unwrap();
        let r3 = rx3.await.unwrap();

        assert!(matches!(
            r1,
            IpcResponse::Pong {
                request_id: 0,
                timestamp: 1
            }
        ));
        assert!(matches!(
            r2,
            IpcResponse::Pong {
                request_id: 0,
                timestamp: 2
            }
        ));
        assert!(matches!(
            r3,
            IpcResponse::Pong {
                request_id: 0,
                timestamp: 3
            }
        ));
    }

    #[tokio::test]
    async fn test_clone_and_share() {
        let tracker = InFlightRequests::new(Duration::from_secs(5));
        let tracker_clone = tracker.clone();

        // Register with original
        let (id1, rx1) = tracker.register();

        // Complete with clone (sync call)
        tracker_clone.complete(
            id1,
            IpcResponse::Pong {
                request_id: 0,
                timestamp: 42,
            },
        );

        // Should receive the response
        let r1 = rx1.await.unwrap();
        assert!(matches!(
            r1,
            IpcResponse::Pong {
                request_id: 0,
                timestamp: 42
            }
        ));
    }
}
