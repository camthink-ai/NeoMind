//! Test/harness source adapters with call counting.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::error::{AgentError, Result};
use crate::ports::{SourceReadPort, SourceReadResult};

#[derive(Clone, Default)]
pub struct CountingSourcePort {
    inner: Arc<Mutex<CountingInner>>,
}

#[derive(Default)]
struct CountingInner {
    /// extension_id → operation → response
    extensions: HashMap<String, HashMap<String, SourceReadResult>>,
    /// device_id → field → response
    devices: HashMap<String, HashMap<String, SourceReadResult>>,
    /// Forced errors: "ext:id:op" or "dev:id:field"
    errors: HashMap<String, String>,
    pub weather_reads: u64,
    pub tariff_reads: u64,
    pub other_reads: u64,
    pub extension_writes: u64,
}

impl CountingSourcePort {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_extension(&self, extension_id: &str, operation: &str, result: SourceReadResult) {
        let mut g = self.inner.lock().expect("lock");
        g.extensions
            .entry(extension_id.into())
            .or_default()
            .insert(operation.into(), result);
    }

    pub fn set_device(&self, device_id: &str, field: &str, result: SourceReadResult) {
        let mut g = self.inner.lock().expect("lock");
        g.devices
            .entry(device_id.into())
            .or_default()
            .insert(field.into(), result);
    }

    pub fn set_extension_error(&self, extension_id: &str, operation: &str, reason: &str) {
        let mut g = self.inner.lock().expect("lock");
        g.errors
            .insert(format!("ext:{extension_id}:{operation}"), reason.into());
    }

    pub fn weather_reads(&self) -> u64 {
        self.inner.lock().expect("lock").weather_reads
    }

    pub fn tariff_reads(&self) -> u64 {
        self.inner.lock().expect("lock").tariff_reads
    }

    pub fn other_reads(&self) -> u64 {
        self.inner.lock().expect("lock").other_reads
    }

    pub fn reset_counts(&self) {
        let mut g = self.inner.lock().expect("lock");
        g.weather_reads = 0;
        g.tariff_reads = 0;
        g.other_reads = 0;
        g.extension_writes = 0;
    }
}

#[async_trait]
impl SourceReadPort for CountingSourcePort {
    async fn read_extension(
        &self,
        extension_id: &str,
        operation: &str,
        _params: &serde_json::Value,
        _timeout_secs: u32,
    ) -> Result<SourceReadResult> {
        let mut g = self.inner.lock().expect("lock");
        let key = format!("ext:{extension_id}:{operation}");
        let err_reason = g.errors.get(&key).cloned();
        bump_read(&mut g, extension_id, operation);
        if let Some(reason) = err_reason {
            return Err(AgentError::Source {
                key: extension_id.into(),
                reason,
            });
        }
        g.extensions
            .get(extension_id)
            .and_then(|m| m.get(operation))
            .cloned()
            .ok_or_else(|| AgentError::Source {
                key: extension_id.into(),
                reason: format!("no fixture for {extension_id}:{operation}"),
            })
    }

    async fn read_device_metric(
        &self,
        device_id: &str,
        field: &str,
        _params: &serde_json::Value,
        _timeout_secs: u32,
    ) -> Result<SourceReadResult> {
        let mut g = self.inner.lock().expect("lock");
        g.other_reads += 1;
        g.devices
            .get(device_id)
            .and_then(|m| m.get(field))
            .cloned()
            .ok_or_else(|| AgentError::Source {
                key: format!("{device_id}:{field}"),
                reason: "no device fixture".into(),
            })
    }
}

fn bump_read(g: &mut CountingInner, extension_id: &str, operation: &str) {
    let id_l = extension_id.to_ascii_lowercase();
    let op_l = operation.to_ascii_lowercase();
    if id_l.contains("weather") || (op_l == "current" && !id_l.contains("tariff")) {
        if id_l.contains("tariff") || id_l.contains("price") {
            g.tariff_reads += 1;
        } else {
            g.weather_reads += 1;
        }
    } else if id_l.contains("tariff") || id_l.contains("price") || op_l.contains("tou") {
        g.tariff_reads += 1;
    } else {
        g.other_reads += 1;
    }
}
