//! Timing and correctness assertions for extension tests.
//!
//! These catch the classes of bugs that plain unit tests miss:
//! - Deadlocks (code hangs, timing assertion fails)
//! - Blocking IO in async context (exceeds wall-clock budget)
//! - Event processing backpressure (runner channel stalls)

use std::time::Duration;

/// Assert a future completes within a time budget.
/// On failure, includes elapsed time and budget for diagnosis.
#[macro_export]
macro_rules! assert_completes_within {
    ($timeout:expr, $future:expr, $context:expr) => {
        match tokio::time::timeout($timeout, $future).await {
            Ok(result) => result,
            Err(_) => panic!(
                "TIMING VIOLATION: {} did not complete within {:?} — \
                 possible deadlock, blocking IO in async context, or lock contention",
                $context, $timeout
            ),
        }
    };
    ($timeout:expr, $future:expr) => {
        assert_completes_within!($timeout, $future, "operation")
    };
}

/// Assert that a command does NOT deadlock when called after another
/// operation that holds locks.
#[macro_export]
macro_rules! assert_no_deadlock {
    ($first_op:expr, $second_op:expr, $context:expr) => {{
        let first = tokio::spawn($first_op);
        // Give the first operation time to acquire its locks
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let second = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            $second_op,
        );
        let _ = first.await;
        match second.await {
            Ok(result) => result,
            Err(_) => panic!(
                "DEADLOCK DETECTED: {} — the second operation could not \
                 complete while the first was in flight",
                $context
            ),
        }
    }};
}

/// Timing violation details for programmatic assertion.
#[derive(Debug)]
pub struct TimingViolation {
    pub context: String,
    pub elapsed: Duration,
    pub budget: Duration,
}

impl std::fmt::Display for TimingViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TIMING VIOLATION: '{}' took {:?} (budget {:?})",
            self.context, self.elapsed, self.budget
        )
    }
}

/// Helper: assert event processing stays under budget.
/// Returns the processing duration for further assertions.
pub async fn assert_event_processed_within<F, Fut>(
    budget: Duration,
    event_type: &str,
    f: F,
) -> Result<Duration, TimingViolation>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(budget, f())
        .await
        .map_err(|_| TimingViolation {
            context: format!("event '{}' processing", event_type),
            elapsed: start.elapsed(),
            budget,
        })?
        .map_err(|e| TimingViolation {
            context: format!("event '{}' returned error: {}", event_type, e),
            elapsed: start.elapsed(),
            budget,
        })?;
    let elapsed = start.elapsed();
    if elapsed > budget {
        return Err(TimingViolation {
            context: format!("event '{}' exceeded budget", event_type),
            elapsed,
            budget,
        });
    }
    Ok(elapsed)
}
