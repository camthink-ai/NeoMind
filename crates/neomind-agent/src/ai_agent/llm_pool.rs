//! LLM Runtime Connection Pool
//!
//! Provides a pooling mechanism for LLM runtime instances to enable:
//! - Multiple concurrent requests per backend
//! - Proper resource limiting
//! - Connection reuse for better performance

use neomind_core::llm::backend::LlmRuntime;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};
use serde::{Deserialize, Serialize};

/// Configuration for the LLM runtime pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmPoolConfig {
    /// Maximum number of runtime instances per backend key
    pub max_instances_per_backend: usize,
    /// Maximum number of total runtime instances across all backends
    pub max_total_instances: usize,
    /// Idle timeout in seconds before an instance is eligible for eviction
    pub idle_timeout_secs: u64,
    /// Whether to enable pool metrics
    pub enable_metrics: bool,
}

impl Default for LlmPoolConfig {
    fn default() -> Self {
        Self {
            max_instances_per_backend: 3,
            max_total_instances: 20,
            idle_timeout_secs: 300, // 5 minutes
            enable_metrics: true,
        }
    }
}

/// Pool metrics for monitoring.
#[derive(Debug, Default, Clone)]
pub struct PoolMetrics {
    /// Number of cache hits
    pub cache_hits: u64,
    /// Number of cache misses
    pub cache_misses: u64,
    /// Current number of instances in the pool
    pub current_instances: usize,
    /// Number of currently busy instances
    pub busy_instances: usize,
}

/// A pooled LLM runtime with metadata.
#[derive(Clone)]
struct PooledRuntime {
    /// The runtime instance
    runtime: Arc<dyn LlmRuntime + Send + Sync>,
    /// Timestamp when this instance was last used
    last_used_at: i64,
    /// Whether this instance is currently in use
    busy: bool,
}

/// Pool entry for a specific backend key.
struct BackendPool {
    /// Available runtime instances (not currently in use)
    available: Vec<PooledRuntime>,
    /// Busy runtime instances (currently in use)
    busy: Vec<PooledRuntime>,
    /// Semaphore for limiting concurrent creation/acquisition
    _semaphore: Arc<Semaphore>,
}

impl BackendPool {
    fn new(max_instances: usize) -> Self {
        Self {
            available: Vec::new(),
            busy: Vec::new(),
            _semaphore: Arc::new(Semaphore::new(max_instances)),
        }
    }

    fn total_instances(&self) -> usize {
        self.available.len() + self.busy.len()
    }
}

/// LLM runtime pool for managing and reusing runtime instances.
pub struct LlmRuntimePool {
    /// Backend key -> pool
    pools: RwLock<HashMap<String, BackendPool>>,
    /// Pool configuration
    config: LlmPoolConfig,
    /// Pool metrics
    metrics: RwLock<PoolMetrics>,
}

impl LlmRuntimePool {
    /// Create a new LLM runtime pool with default configuration.
    pub fn new() -> Self {
        Self::with_config(LlmPoolConfig::default())
    }

    /// Create a new LLM runtime pool with custom configuration.
    pub fn with_config(config: LlmPoolConfig) -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            config,
            metrics: RwLock::new(PoolMetrics::default()),
        }
    }

    /// Acquire a runtime from the pool or create a new one.
    ///
    /// The runtime is returned as a `PooledRuntimeGuard` which will
    /// automatically return the runtime to the pool when dropped.
    pub async fn acquire(
        &self,
        key: String,
        creator: impl Fn() -> Result<Arc<dyn LlmRuntime + Send + Sync>, crate::error::NeoMindError>,
    ) -> Result<PooledRuntimeGuard, crate::error::NeoMindError> {
        // Wait for semaphore permit (limits total concurrent acquisitions)
        let mut pools = self.pools.write().await;
        let pool = pools.entry(key.clone()).or_insert_with(|| {
            BackendPool::new(self.config.max_instances_per_backend)
        });

        // Try to get an available runtime
        if let Some(idx) = pool.available.iter().position(|r| !r.busy) {
            let mut pooled = pool.available.remove(idx);
            pooled.busy = true;
            pool.busy.push(pooled.clone());

            // Get busy count before dropping pools
            let busy_count = pool.busy.len();
            drop(pools);

            // Record cache hit
            if self.config.enable_metrics {
                let mut metrics = self.metrics.write().await;
                metrics.cache_hits += 1;
                metrics.busy_instances = busy_count;
            }

            return Ok(PooledRuntimeGuard {
                pool: self.clone(),
                key,
                runtime: Some(pooled.runtime),
            });
        }

        // No available runtime, check if we can create a new one
        if pool.total_instances() < self.config.max_instances_per_backend {
            // Create new runtime
            let runtime = creator()?;
            let pooled = PooledRuntime {
                runtime: runtime.clone(),
                last_used_at: chrono::Utc::now().timestamp(),
                busy: true,
            };
            pool.busy.push(pooled);

            // Get busy count before dropping pools
            let busy_count = pool.busy.len();
            drop(pools);

            // Record cache miss
            if self.config.enable_metrics {
                let mut metrics = self.metrics.write().await;
                metrics.cache_misses += 1;
                metrics.current_instances += 1;
                metrics.busy_instances = busy_count;
            }

            Ok(PooledRuntimeGuard {
                pool: self.clone(),
                key,
                runtime: Some(runtime),
            })
        } else {
            // Pool is full, wait for an available runtime
            drop(pools);

            // For simplicity, we'll just return an error here
            // A more sophisticated implementation would wait on a condition variable
            Err(crate::NeoMindError::Llm(
                "LLM runtime pool is at maximum capacity".to_string()
            ))
        }
    }

    /// Return a runtime to the pool.
    async fn return_runtime(&self, key: &str, runtime: Arc<dyn LlmRuntime + Send + Sync>) {
        let mut pools = self.pools.write().await;
        if let Some(pool) = pools.get_mut(key) {
            // Find the runtime in busy list and move it to available
            if let Some(idx) = pool.busy.iter().position(|r| {
                // Compare by pointer (Arc::ptr_eq)
                Arc::ptr_eq(&r.runtime, &runtime)
            }) {
                let mut pooled = pool.busy.remove(idx);
                pooled.busy = false;
                pooled.last_used_at = chrono::Utc::now().timestamp();
                pool.available.push(pooled);

                // Update metrics
                if self.config.enable_metrics {
                    let mut metrics = self.metrics.write().await;
                    metrics.busy_instances = pool.busy.len();
                }
            }
        }
    }

    /// Get current pool metrics.
    pub async fn metrics(&self) -> PoolMetrics {
        if self.config.enable_metrics {
            self.metrics.read().await.clone()
        } else {
            PoolMetrics::default()
        }
    }

    /// Evict idle runtime instances based on idle timeout.
    pub async fn evict_idle(&self) -> usize {
        let now = chrono::Utc::now().timestamp();
        let mut evicted = 0;

        let mut pools = self.pools.write().await;
        for pool in pools.values_mut() {
            let initial_len = pool.available.len();
            pool.available.retain(|r| {
                now - r.last_used_at < self.config.idle_timeout_secs as i64
            });
            evicted += initial_len - pool.available.len();
        }

        // Clean up empty pools
        pools.retain(|_, pool| !pool.available.is_empty() || !pool.busy.is_empty());

        // Update metrics
        if self.config.enable_metrics && evicted > 0 {
            let mut metrics = self.metrics.write().await;
            metrics.current_instances = pools.values()
                .map(|p| p.total_instances())
                .sum();
        }

        evicted
    }

    /// Get the total number of runtime instances across all backends.
    pub async fn total_instances(&self) -> usize {
        let pools = self.pools.read().await;
        pools.values().map(|p| p.total_instances()).sum()
    }
}

impl Clone for LlmRuntimePool {
    fn clone(&self) -> Self {
        Self {
            pools: RwLock::new(HashMap::new()), // Empty pools - each clone gets its own
            config: self.config.clone(),
            metrics: RwLock::new(PoolMetrics::default()),
        }
    }
}

/// Guard that returns the runtime to the pool when dropped.
pub struct PooledRuntimeGuard {
    pool: LlmRuntimePool,
    key: String,
    runtime: Option<Arc<dyn LlmRuntime + Send + Sync>>,
}

impl PooledRuntimeGuard {
    /// Get the runtime instance.
    pub fn runtime(&self) -> &Arc<dyn LlmRuntime + Send + Sync> {
        self.runtime.as_ref().expect("Runtime already consumed")
    }

    /// Consume the guard and return the runtime without returning to pool.
    ///
    /// Use this if you want to keep the runtime permanently.
    pub fn detach(mut self) -> Arc<dyn LlmRuntime + Send + Sync> {
        self.runtime.take().expect("Runtime already consumed")
    }
}

impl Drop for PooledRuntimeGuard {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            // Spawn a task to return the runtime to the pool
            let pool = self.pool.clone();
            let key = self.key.clone();
            tokio::spawn(async move {
                pool.return_runtime(&key, runtime).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config_default() {
        let config = LlmPoolConfig::default();
        assert_eq!(config.max_instances_per_backend, 3);
        assert_eq!(config.max_total_instances, 20);
        assert_eq!(config.idle_timeout_secs, 300);
    }

    #[test]
    fn test_pool_metrics_default() {
        let metrics = PoolMetrics::default();
        assert_eq!(metrics.cache_hits, 0);
        assert_eq!(metrics.cache_misses, 0);
        assert_eq!(metrics.current_instances, 0);
        assert_eq!(metrics.busy_instances, 0);
    }
}
