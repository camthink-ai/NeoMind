# NeoMind Extension System Stability Fix Plan
**Created**: 2026-03-16
**Target**: Production-ready extension system
**Estimated Effort**: 4-6 weeks
**Priority**: 🔴 CRITICAL

---

## 📊 Current Status

### Completed (This Iteration)
- ✅ Fixed serde_json ARM NEON crash (SDK feature flag)
- ✅ Removed deprecated llm_hints field
- ✅ All extension-specific issues resolved

### Remaining Blockers
- ❌ Circular dependency (core ↔ SDK)
- ❌ Zombie process accumulation
- ❌ No crash recovery mechanism
- ❌ IPC communication fragility

---

## 🎯 Phase 1: Critical Stability Fixes (Week 1)

### Goal
Eliminate resource leaks and add basic crash recovery

### Tasks

#### 1.1 Fix Zombie Process Leak (P0 - CRITICAL)
**Location**: `crates/neomind-core/src/extension/isolated/process.rs`

**Problem**:
```rust
impl Drop for ExtensionProcess {
    fn drop(&mut self) {
        // Cannot call wait() here - would block
        // Child process becomes zombie
    }
}
```

**Solution**:
```rust
// Add background cleanup task
pub struct ExtensionProcess {
    child: Arc<Mutex<Option<Child>>>,
    _cleanup_task: JoinHandle<()>,
}

impl ExtensionProcess {
    pub fn spawn(mut cmd: Command) -> Result<Self> {
        let child = cmd.spawn()?;
        let child_arc = Arc::new(Mutex::new(Some(child)));
        
        // Spawn cleanup task
        let child_clone = Arc::clone(&child_arc);
        let cleanup_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let mut child_guard = child_clone.lock().await;
                if let Some(child) = child_guard.as_mut() {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            tracing::info!("Extension process exited: {:?}", status);
                            *child_guard = None;
                            break;
                        }
                        Ok(None) => continue,
                        Err(e) => {
                            tracing::error!("Failed to wait for child: {}", e);
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
        });
        
        Ok(Self {
            child: child_arc,
            _cleanup_task: cleanup_task,
        })
    }
}
```

**Testing**:
- [ ] Upload and unload extension 100 times
- [ ] Monitor process table (no defunct processes)
- [ ] Check memory usage over time

**Estimated**: 4 hours
**Risk**: Medium

---

#### 1.2 Add Crash Detection & Notification (P0 - CRITICAL)
**Location**: `crates/neomind-core/src/extension/isolated/manager.rs`

**Problem**: No notification when extension crashes

**Solution**:
```rust
pub struct ExtensionManager {
    // ... existing fields ...
    crash_tx: mpsc::UnboundedSender<CrashEvent>,
}

#[derive(Debug, Clone)]
pub struct CrashEvent {
    pub extension_id: String,
    pub timestamp: SystemTime,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
}

impl ExtensionManager {
    pub async fn spawn_with_monitoring(
        &self,
        extension_id: String,
    ) -> Result<ExtensionHandle> {
        let process = self.spawn_process(&extension_id).await?;
        
        // Monitor process health
        let crash_tx = self.crash_tx.clone();
        tokio::spawn(async move {
            let status = process.wait().await;
            let event = CrashEvent {
                extension_id: extension_id.clone(),
                timestamp: SystemTime::now(),
                exit_code: status.code(),
                signal: status.signal(),
            };
            
            let _ = crash_tx.send(event);
            
            // Log crash
            tracing::error!(
                extension_id = %extension_id,
                ?status,
                "Extension process crashed"
            );
        });
        
        Ok(process)
    }
}
```

**Testing**:
- [ ] Intentionally crash extension (panic!)
- [ ] Verify crash event is emitted
- [ ] Check logs contain crash details

**Estimated**: 6 hours
**Risk**: Low

---

#### 1.3 Add Automatic Restart on Crash (P1 - HIGH)
**Location**: `crates/neomind-core/src/extension/isolated/manager.rs`

**Problem**: Extensions don't restart after crash

**Solution**:
```rust
pub struct RestartPolicy {
    max_restarts: usize,
    backoff_duration: Duration,
}

impl ExtensionManager {
    pub async fn spawn_with_restart(
        &self,
        extension_id: String,
        policy: RestartPolicy,
    ) -> Result<ExtensionHandle> {
        let mut restart_count = 0;
        
        loop {
            match self.spawn_with_monitoring(extension_id.clone()).await {
                Ok(handle) => {
                    // Monitor for crashes
                    let mut crash_rx = self.crash_tx.subscribe();
                    
                    tokio::select! {
                        _ = handle.cancelled() => break Ok(handle),
                        
                        event = crash_rx.recv() => {
                            if let Some(event) = event {
                                if event.extension_id == extension_id {
                                    restart_count += 1;
                                    
                                    if restart_count >= policy.max_restarts {
                                        tracing::error!(
                                            extension_id = %extension_id,
                                            "Max restarts reached, giving up"
                                        );
                                        break Err(Error::MaxRestartsExceeded);
                                    }
                                    
                                    tracing::warn!(
                                        extension_id = %extension_id,
                                        restart_count,
                                        "Restarting extension after crash"
                                    );
                                    
                                    tokio::time::sleep(policy.backoff_duration).await;
                                    continue;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }
}
```

**Testing**:
- [ ] Crash extension, verify it restarts
- [ ] Test max restarts limit
- [ ] Test backoff duration

**Estimated**: 4 hours
**Risk**: Medium

---

#### 1.4 Improve IPC Error Messages (P1 - HIGH)
**Location**: `crates/neomind-core/src/extension/isolated/process.rs`

**Problem**: "Failed to read from extension stdout" is not helpful

**Solution**:
```rust
match stdout.read_exact(&mut len_bytes) {
    Ok(()) => {}
    Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
        error!(
            extension_id = %extension_id,
            "Extension process terminated unexpectedly (stdout closed)"
        );
        running.store(false, Ordering::SeqCst);
        self.notify_crash(ExtensionCrashReason::UnexpectedEof).await;
        break;
    }
    Err(e) => {
        warn!(
            extension_id = %extension_id,
            error = %e,
            "Failed to read from extension stdout"
        );
        running.store(false, Ordering::SeqCst);
        break;
    }
}
```

**Testing**:
- [ ] Trigger various IPC failures
- [ ] Verify error messages are specific
- [ ] Check crash notifications include details

**Estimated**: 2 hours
**Risk**: Low

---

### Phase 1 Deliverables
- [ ] No zombie processes after 100 extension loads/unloads
- [ ] Crash events logged with details (exit code, signal)
- [ ] Extensions auto-restart (max 3 times)
- [ ] Clear error messages in logs
- [ ] Memory stable over 8 hours

**Estimated Total**: 16 hours (2 days)

---

## 🏗️ Phase 2: Architecture Cleanup (Weeks 2-3)

### Goal
Break circular dependency and improve system architecture

### Tasks

#### 2.1 Create neomind-extension-types Crate (P0 - CRITICAL)
**New Crate**: `crates/neomind-extension-types/`

**Purpose**: Break circular dependency between core and SDK

**Structure**:
```
crates/neomind-extension-types/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── extension.rs       // ExtensionMetadata, ExtensionError
│   ├── commands.rs        // CommandDefinition, ParameterDefinition
│   ├── metrics.rs         // MetricDescriptor, MetricDataType
│   ├── parameters.rs      // ParamMetricValue
│   └── context.rs         // ExtensionContext
└── tests/
    └── basic_tests.rs
```

**Implementation**:
```rust
// Cargo.toml
[package]
name = "neomind-extension-types"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
semver = "1"
```

```rust
// src/lib.rs
pub mod extension;
pub mod commands;
pub mod metrics;
pub mod parameters;
pub mod context;

// Re-export all types
pub use extension::*;
pub use commands::*;
pub use metrics::*;
pub use parameters::*;
pub use context::*;
```

**Migration Steps**:
1. Create new crate
2. Move type definitions from core and SDK
3. Update core to depend on types
4. Update SDK to depend on types
5. Remove circular dependency

**Testing**:
- [ ] Compiles without circular dependency
- [ ] All tests pass
- [ ] Extensions still load correctly
- [ ] No API breaks (re-exports for compatibility)

**Estimated**: 16 hours (2 days)
**Risk**: High (architecture change)

---

#### 2.2 Improve IPC Robustness (P1 - HIGH)
**Location**: `crates/neomind-core/src/extension/isolated/process.rs`

**Problem**: IPC failures cause silent crashes

**Solution**:
```rust
// Add timeout and retry
pub struct IpcConfig {
    pub read_timeout: Duration,
    pub max_retries: usize,
}

impl ExtensionProcess {
    async fn send_request_with_retry(
        &self,
        request: IpcRequest,
        config: &IpcConfig,
    ) -> Result<IpcResponse> {
        let mut retries = 0;
        
        loop {
            tokio::select! {
                result = tokio::time::timeout(
                    config.read_timeout,
                    self.send_request(request.clone())
                ) => {
                    match result {
                        Ok(Ok(response)) => return Ok(response),
                        Ok(Err(e)) => {
                            if retries >= config.max_retries {
                                return Err(e);
                            }
                            retries += 1;
                            tokio::time::sleep(Duration::from_millis(100 * retries as u64)).await;
                            continue;
                        }
                        Err(_) => {
                            // Timeout
                            if retries >= config.max_retries {
                                return Err(Error::Timeout);
                            }
                            retries += 1;
                            continue;
                        }
                    }
                }
            }
        }
    }
}
```

**Testing**:
- [ ] Test with slow extensions
- [ ] Test with failing extensions
- [ ] Verify retries work
- [ ] Check timeout enforcement

**Estimated**: 6 hours
**Risk**: Medium

---

#### 2.3 Add Extension Health Monitoring (P1 - HIGH)
**Location**: `crates/neomind-core/src/extension/monitoring.rs` (new)

**Problem**: No visibility into extension health

**Solution**:
```rust
pub struct ExtensionHealthMonitor {
    extensions: Arc<RwLock<HashMap<String, ExtensionHealth>>>,
}

#[derive(Debug, Clone)]
pub struct ExtensionHealth {
    pub pid: u32,
    pub memory_mb: f64,
    pub cpu_percent: f64,
    pub uptime_seconds: u64,
    pub last_heartbeat: SystemTime,
    pub status: ExtensionStatus,
}

#[derive(Debug, Clone)]
pub enum ExtensionStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Crashed,
}

impl ExtensionHealthMonitor {
    pub async fn start_monitoring(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        
        loop {
            interval.tick().await;
            self.check_all_extensions().await;
        }
    }
    
    async fn check_all_extensions(&self) {
        let extensions = self.extensions.read().await;
        
        for (id, health) in extensions.iter() {
            // Check if process is still alive
            if let Ok(true) = Self::is_process_alive(health.pid) {
                // Update metrics
                self.update_metrics(id).await;
            } else {
                // Process died
                self.handle_crash(id).await;
            }
        }
    }
}
```

**Testing**:
- [ ] Monitor reports correct health status
- [ ] Memory/CPU metrics accurate
- [ ] Crashes detected quickly
- [ ] Health endpoint works

**Estimated**: 8 hours
**Risk**: Low

---

#### 2.4 Add Comprehensive Logging (P2 - MEDIUM)
**Location**: Throughout extension system

**Problem**: Insufficient logging for debugging

**Solution**:
```rust
// Add structured logging
use tracing::{info, warn, error, debug, instrument};

#[instrument(skip(self))]
impl ExtensionManager {
    pub async fn load_extension(&self, package_path: &Path) -> Result<()> {
        info!(
            package_path = %package_path.display(),
            "Loading extension package"
        );
        
        let package = self.load_package(package_path).await?;
        debug!(?package, "Package loaded");
        
        let process = self.spawn_process(&package.id).await?;
        info!(
            extension_id = %package.id,
            pid = process.id(),
            "Extension process started"
        );
        
        // ... rest of implementation
        
        info!(
            extension_id = %package.id,
            "Extension loaded successfully"
        );
        
        Ok(())
    }
}
```

**Testing**:
- [ ] All major operations logged
- [ ] Logs contain context (extension_id, pid)
- [ ] Log levels appropriate (info/warn/error)
- [ ] Performance impact minimal

**Estimated**: 6 hours
**Risk**: Low

---

### Phase 2 Deliverables
- [ ] No circular dependency in build graph
- [ ] IPC has timeout and retry logic
- [ ] Health monitoring active
- [ ] Comprehensive logging
- [ ] Documentation updated

**Estimated Total**: 36 hours (4-5 days)

---

## 🧪 Phase 3: Production Hardening (Week 4)

### Goal
Ensure system is production-ready with comprehensive testing

### Tasks

#### 3.1 Stress Testing (P0 - CRITICAL)
**Test Cases**:
- [ ] Load 100 extensions sequentially
- [ ] Load 50 extensions concurrently
- [ ] Crash and reload extensions 1000 times
- [ ] Run for 24 hours without restart
- [ ] Monitor memory leaks
- [ ] Monitor zombie processes

**Tools**:
```bash
# Stress test script
./scripts/stress_test_extensions.sh \
  --extensions 100 \
  --concurrent 10 \
  --cycles 1000 \
  --duration 24h
```

**Success Criteria**:
- No zombie processes
- Memory usage stable (< 500MB)
- No crashes after 1000 load/unload cycles
- All extensions recover from crashes

**Estimated**: 8 hours

---

#### 3.2 Load Testing (P1 - HIGH)
**Test Cases**:
- [ ] 10 extensions running simultaneously
- [ ] Each extension processes 100 req/sec
- [ ] Test for 1 hour sustained load
- [ ] Measure latency percentiles (p50, p95, p99)
- [ ] Check for deadlocks

**Tools**:
- Apache Bench (ab)
- wrk
- Custom load generator

**Success Criteria**:
- p95 latency < 100ms
- No errors under load
- CPU usage < 80%
- No deadlocks or hangs

**Estimated**: 6 hours

---

#### 3.3 Failover Testing (P2 - MEDIUM)
**Test Cases**:
- [ ] Kill extension process, verify restart
- [ ] Kill NeoMind process, verify extensions reload
- [ ] Network partition (if applicable)
- [ ] Disk full scenarios
- [ ] Out of memory scenarios

**Success Criteria**:
- Extensions restart automatically
- No data loss
- Graceful degradation
- Clear error messages

**Estimated**: 4 hours

---

#### 3.4 Security Testing (P1 - HIGH)
**Test Cases**:
- [ ] Malicious extension package upload
- [ ] Extension attempts to escape sandbox
- [ ] Memory exhaustion attacks
- [ ] CPU exhaustion attacks
- [ ] File system access violations

**Tools**:
- Valgrind
- Address Sanitizer
- Custom security tests

**Success Criteria**:
- Malicious extensions rejected
- Sandbox enforced
- Resource limits enforced
- No privilege escalation

**Estimated**: 8 hours

---

#### 3.5 Documentation Updates (P2 - MEDIUM)
**Documents**:
- [ ] Extension development guide
- [ ] Deployment guide
- [ ] Troubleshooting guide
- [ ] API reference updates
- [ ] Architecture diagrams
- [ ] Known issues and workarounds

**Locations**:
- `docs/guides/en/16-extension-dev.md`
- `docs/guides/zh/16-extension-dev.md`
- `docs/troubleshooting.md`
- `docs/architecture/extensions.md`

**Estimated**: 8 hours

---

### Phase 3 Deliverables
- [ ] Stress test report
- [ ] Load test results
- [ ] Failover test results
- [ ] Security audit report
- [ ] Updated documentation
- [ ] Production readiness checklist

**Estimated Total**: 34 hours (4-5 days)

---

## 📊 Overall Timeline

```
Week 1: Phase 1 - Critical Stability (16 hours)
  ├─ Mon-Tue: Zombie process fix + crash detection
  ├─ Wed: Automatic restart
  └─ Thu-Fri: Error messages + testing

Week 2-3: Phase 2 - Architecture Cleanup (36 hours)
  ├─ Week 2: Create types crate, remove circular dep
  └─ Week 3: IPC improvements + health monitoring + logging

Week 4: Phase 3 - Production Hardening (34 hours)
  ├─ Mon-Tue: Stress + load testing
  ├─ Wed: Security testing
  └─ Thu-Fri: Documentation + final checks

Total: 4-6 weeks
```

---

## 🎯 Success Criteria

### Must Have (P0)
- [ ] No zombie processes
- [ ] Automatic crash recovery
- [ ] No circular dependencies
- [ ] Pass stress tests (1000 cycles)

### Should Have (P1)
- [ ] Health monitoring
- [ ] Comprehensive logging
- [ ] IPC timeout/retry
- [ ] Security audit passed

### Nice to Have (P2)
- [ ] Performance optimization
- [ ] Advanced metrics
- [ ] Debugging tools
- [ ] Extension templates

---

## 📝 Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Breaking changes | Medium | High | Maintain compatibility layer |
| Performance regression | Low | Medium | Benchmark before/after |
| New bugs introduced | Medium | High | Comprehensive testing |
| Timeline overrun | High | Low | Phased rollout, defer P2 items |

---

## 🚀 Rollout Plan

### Stage 1: Alpha (Internal)
- Deploy to development environment
- Test with 4 extensions
- Run for 1 week
- Collect feedback

### Stage 2: Beta (Limited Users)
- Deploy to staging environment
- Invite 5-10 power users
- Run for 2 weeks
- Fix critical bugs

### Stage 3: Production
- Deploy to production
- Monitor closely for 1 week
- Be ready to rollback
- Phase 1 extensions first, then others

---

## 📋 Checklist

### Before Starting
- [ ] All stakeholders briefed
- [ ] Timeline approved
- [ ] Resources allocated
- [ ] Test environment ready
- [ ] Backup plan prepared

### During Implementation
- [ ] Daily standups
- [ ] Weekly progress reports
- [ ] Code reviews for all changes
- [ ] Tests updated with each change
- [ ] Documentation kept in sync

### Before Release
- [ ] All P0 issues resolved
- [ ] All P1 issues resolved or deferred
- [ ] Tests passing (unit + integration)
- [ ] Security audit complete
- [ ] Performance benchmarks met
- [ ] Documentation complete
- [ ] Rollback plan tested

---

**Created by**: Claude Code AI
**Last updated**: 2026-03-16
**Status**: 📋 Planning Phase
**Next action**: Review and approve plan
