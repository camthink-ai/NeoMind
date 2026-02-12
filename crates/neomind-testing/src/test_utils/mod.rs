//! Production-grade test utilities
//!
//! This module provides utilities for creating isolated test environments
//! that can run in parallel without interfering with each other.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global test counter for generating unique test IDs
static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Get a unique test ID for this test run
pub fn test_id() -> String {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("test_{}", id)
}

/// Get a unique temporary directory for this test
pub fn test_temp_dir() -> PathBuf {
    let id = test_id();
    let mut path = std::env::temp_dir();
    path.push(format!("neomind_test_{}", id));
    path
}

/// Clean up test directory
pub fn cleanup_test_dir(path: &PathBuf) {
    let _ = std::fs::remove_dir_all(path);
}

/// Test database configuration
pub struct TestDbConfig {
    pub path: PathBuf,
    pub cleanup_on_drop: bool,
}

impl TestDbConfig {
    /// Create a new test database configuration with unique path
    pub fn new(name: &str) -> Self {
        let id = test_id();
        let mut path = std::env::temp_dir();
        path.push(format!("neomind_{}_{}.redb", name, id));
        Self {
            path,
            cleanup_on_drop: true,
        }
    }

    /// Create an in-memory test database configuration
    pub fn memory() -> Self {
        Self {
            path: PathBuf::from(":memory:"),
            cleanup_on_drop: false,
        }
    }
}

impl Drop for TestDbConfig {
    fn drop(&mut self) {
        if self.cleanup_on_drop && self.path.to_string_lossy() != ":memory:" {
            let _ = std::fs::remove_file(&self.path);
            let lock_path = self.path.with_extension("lock");
            let _ = std::fs::remove_file(&lock_path);
        }
    }
}

/// Retry helper for flaky tests
pub async fn retry<R, E, F, Fut>(mut f: F, max_attempts: u32) -> Result<R, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<R, E>>,
{
    let mut last_error = None;
    for attempt in 1..=max_attempts {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = Some(e);
                if attempt < max_attempts {
                    tokio::time::sleep(std::time::Duration::from_millis(100 * attempt as u64)).await;
                }
            }
        }
    }
    Err(last_error.unwrap())
}

/// Assert with retry logic
#[macro_export]
macro_rules! assert_eventually {
    ($condition:expr, $max_attempts:expr $(,)?) => {{
        let mut attempts = 0;
        loop {
            match ($condition) {
                true => break,
                false if attempts >= $max_attempts => {
                    panic!("Condition did not become true after {} attempts", $max_attempts);
                }
                false => {
                    attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_id_is_unique() {
        let id1 = test_id();
        let id2 = test_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_test_temp_dir_is_unique() {
        let dir1 = test_temp_dir();
        let dir2 = test_temp_dir();
        assert_ne!(dir1, dir2);
    }

    #[tokio::test]
    async fn test_retry_success_immediately() {
        // Test that retry works when the first attempt succeeds
        let result = retry(|| async { Ok::<(), &str>(()) }, 3).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[should_panic]
    async fn test_retry_fails_after_max_attempts() {
        retry(|| async { Err::<(), _>("always fails") }, 3).await;
    }
}
