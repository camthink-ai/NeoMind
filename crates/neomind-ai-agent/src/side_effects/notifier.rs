//! Host notification delivery with fake receiver for tests.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::NotifyPort;

#[derive(Clone, Default)]
pub struct FakeNotifyPort {
    inner: Arc<Mutex<Vec<NotifyCall>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyCall {
    pub channel: String,
    pub subject: String,
    pub body: String,
    pub execution_id: String,
}

impl FakeNotifyPort {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> u64 {
        self.inner.lock().expect("lock").len() as u64
    }

    pub fn calls(&self) -> Vec<NotifyCall> {
        self.inner.lock().expect("lock").clone()
    }

    pub fn reset(&self) {
        self.inner.lock().expect("lock").clear();
    }
}

#[async_trait]
impl NotifyPort for FakeNotifyPort {
    async fn deliver(
        &self,
        channel: &str,
        subject: &str,
        body: &str,
        execution_id: &str,
    ) -> Result<()> {
        self.inner.lock().expect("lock").push(NotifyCall {
            channel: channel.into(),
            subject: subject.into(),
            body: body.into(),
            execution_id: execution_id.into(),
        });
        Ok(())
    }
}

pub struct Notifier {
    port: Arc<dyn NotifyPort>,
}

impl Notifier {
    pub fn new(port: Arc<dyn NotifyPort>) -> Self {
        Self { port }
    }

    pub async fn deliver(
        &self,
        channel: &str,
        subject: &str,
        body: &str,
        execution_id: &str,
    ) -> Result<()> {
        self.port
            .deliver(channel, subject, body, execution_id)
            .await
    }
}
