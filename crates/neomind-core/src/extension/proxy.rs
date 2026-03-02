//! Proxy extension for isolated extensions
//!
//! This module provides a proxy type that wraps an IsolatedExtension
//! and implements the Extension trait, allowing isolated extensions
//! to be used in contexts that require DynExtension (like streaming).

use std::sync::Arc;
use async_trait::async_trait;

use crate::extension::isolated::{IsolatedExtension, IsolatedExtensionError};
use crate::extension::system::{
    Extension, ExtensionDescriptor, ExtensionError, ExtensionMetadata, ExtensionMetricValue,
    ExtensionCommand, MetricDescriptor, Result,
};
use crate::extension::stream::{
    StreamCapability, StreamSession, DataChunk, StreamResult, SessionStats,
};
use crate::extension::types;

/// Proxy that wraps an IsolatedExtension and implements Extension trait
///
/// This allows isolated extensions to be used in contexts that require
/// DynExtension, such as the streaming handler.
pub struct IsolatedExtensionProxy {
    /// The underlying isolated extension
    isolated: Arc<IsolatedExtension>,
    /// Cached metadata
    cached_metadata: ExtensionMetadata,
    /// Cached commands
    cached_commands: Vec<ExtensionCommand>,
    /// Cached metrics
    cached_metrics: Vec<MetricDescriptor>,
    /// Cached stream capability
    cached_stream_capability: Option<StreamCapability>,
}

impl IsolatedExtensionProxy {
    /// Create a new proxy for an isolated extension
    pub fn new(isolated: Arc<IsolatedExtension>) -> Self {
        // Use tokio::task::block_in_place to get descriptor and stream capability
        let (metadata, commands, metrics, stream_capability) = tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::try_current();
            match rt {
                Ok(rt) => {
                    rt.block_on(async {
                        let desc = isolated.descriptor().await;
                        let cap = isolated.stream_capability().await.ok().flatten();
                        
                        match desc {
                            Some(d) => (d.metadata, d.commands, d.metrics, cap),
                            None => (
                                ExtensionMetadata::new(
                                    "unknown".to_string(),
                                    "Unknown Extension".to_string(),
                                    semver::Version::new(0, 0, 0),
                                ),
                                Vec::new(),
                                Vec::new(),
                                cap,
                            ),
                        }
                    })
                }
                Err(_) => (
                    ExtensionMetadata::new(
                        "unknown".to_string(),
                        "Unknown Extension".to_string(),
                        semver::Version::new(0, 0, 0),
                    ),
                    Vec::new(),
                    Vec::new(),
                    None,
                ),
            }
        });

        Self {
            isolated,
            cached_metadata: metadata,
            cached_commands: commands,
            cached_metrics: metrics,
            cached_stream_capability: stream_capability,
        }
    }

    /// Create with full descriptor
    pub fn with_descriptor(isolated: Arc<IsolatedExtension>, descriptor: ExtensionDescriptor) -> Self {
        let metadata = descriptor.metadata.clone();
        let commands = descriptor.commands.clone();
        let metrics = descriptor.metrics.clone();
        
        // Get stream capability using block_in_place
        let stream_capability = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::try_current()
                .ok()
                .and_then(|rt| rt.block_on(async {
                    isolated.stream_capability().await.ok().flatten()
                }))
        });

        Self {
            isolated,
            cached_metadata: metadata,
            cached_commands: commands,
            cached_metrics: metrics,
            cached_stream_capability: stream_capability,
        }
    }

    /// Convert IsolatedExtensionError to ExtensionError
    fn convert_error(e: IsolatedExtensionError) -> ExtensionError {
        match e {
            IsolatedExtensionError::SpawnFailed(msg) => ExtensionError::LoadFailed(msg),
            IsolatedExtensionError::IpcError(msg) => ExtensionError::ExecutionFailed(msg),
            IsolatedExtensionError::Crashed(msg) => ExtensionError::ExecutionFailed(msg),
            IsolatedExtensionError::Timeout(ms) => ExtensionError::Timeout(format!("{}ms", ms)),
            IsolatedExtensionError::InvalidResponse(msg) => ExtensionError::ExecutionFailed(msg),
            IsolatedExtensionError::NotInitialized => ExtensionError::ExecutionFailed("Extension not initialized".to_string()),
            IsolatedExtensionError::AlreadyRunning => ExtensionError::ExecutionFailed("Extension already running".to_string()),
            IsolatedExtensionError::NotRunning => ExtensionError::ExecutionFailed("Extension not running".to_string()),
            IsolatedExtensionError::LoadError(msg) => ExtensionError::LoadFailed(msg),
            IsolatedExtensionError::UnexpectedResponse => ExtensionError::ExecutionFailed("Unexpected response type".to_string()),
            IsolatedExtensionError::ChannelClosed => ExtensionError::ExecutionFailed("Response channel closed".to_string()),
            IsolatedExtensionError::ExtensionError(msg) => ExtensionError::ExecutionFailed(msg),
        }
    }
}

#[async_trait]
impl Extension for IsolatedExtensionProxy {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.cached_metadata
    }

    fn commands(&self) -> &[ExtensionCommand] {
        &self.cached_commands
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        &self.cached_metrics
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        self.isolated
            .execute_command(command, args)
            .await
            .map_err(Self::convert_error)
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // Use block_in_place with handle to avoid nested runtime issues
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::try_current()
                .map_err(|_| ExtensionError::ExecutionFailed("No tokio runtime".to_string()))?;
            
            rt.block_on(async {
                self.isolated
                    .produce_metrics()
                    .await
                    .map_err(Self::convert_error)
            })
        })
    }

    async fn health_check(&self) -> Result<bool> {
        self.isolated
            .health_check()
            .await
            .map_err(Self::convert_error)
    }

    fn stream_capability(&self) -> Option<StreamCapability> {
        // Return cached capability - no async calls here
        self.cached_stream_capability.clone()
    }

    async fn init_session(&self, session: &StreamSession) -> Result<()> {
        self.isolated
            .init_session(&session.id, session.config.clone())
            .await
            .map_err(Self::convert_error)
    }

    async fn process_session_chunk(&self, session_id: &str, chunk: DataChunk) -> Result<StreamResult> {
        self.isolated
            .process_session_chunk(session_id, chunk)
            .await
            .map_err(Self::convert_error)
    }

    async fn close_session(&self, session_id: &str) -> Result<SessionStats> {
        self.isolated
            .close_session(session_id)
            .await
            .map_err(Self::convert_error)
    }
}

/// Create a DynExtension from an IsolatedExtension
pub fn create_proxy(isolated: Arc<IsolatedExtension>) -> types::DynExtension {
    let proxy = IsolatedExtensionProxy::new(isolated);
    Arc::new(tokio::sync::RwLock::new(Box::new(proxy)))
}

/// Create a DynExtension from an IsolatedExtension with descriptor
pub fn create_proxy_with_descriptor(
    isolated: Arc<IsolatedExtension>,
    descriptor: ExtensionDescriptor,
) -> types::DynExtension {
    let proxy = IsolatedExtensionProxy::with_descriptor(isolated, descriptor);
    Arc::new(tokio::sync::RwLock::new(Box::new(proxy)))
}