//! `runner` — split from the former main.rs monolith.

use super::*;

use std::collections::VecDeque;

use std::io::{Read, Write};

use std::path::{Path, PathBuf};

use std::sync::Arc;

use serde_json::json;

use tracing::{debug, error, info, trace, warn};

use neomind_extension_sdk::{
    BatchCommand, BatchResult, ErrorKind, ExtensionDescriptor, ExtensionMetadata,
    ExtensionMetricValue, ExtensionStats, IpcFrame, IpcMessage, IpcResponse, SessionStats,
    StreamCapability, StreamDataChunk, StreamResult,
};

use dylib_validation::validate_library;

use event_handler::get_global_event_state;

use ipc_routing::{
    complete_pending_request, create_event_channel, start_stdin_reader, STDOUT_WRITE_MUTEX,
};

impl Runner {
    /// Load extension and create runner
    pub(crate) async fn load(extension_path: &PathBuf) -> Result<Self, String> {
        debug!("Runner::load called");
        let extension_type = ExtensionType::from_path(extension_path);
        debug!(
            path = %extension_path.display(),
            extension_type = ?extension_type,
            "Loading extension"
        );

        // Load the extension based on type
        let (extension, wasm_runtime, descriptor) = match extension_type {
            ExtensionType::Native => {
                let (ext, desc) = Self::load_native(extension_path).await?;
                (Some(ext), None, desc)
            }
            ExtensionType::Wasm => {
                let (runtime, descriptor) = Self::load_wasm(extension_path).await?;
                (None, Some(runtime), descriptor)
            }
        };

        debug!(
            commands_count = descriptor.commands.len(),
            "Extension loaded"
        );

        debug!(
            extension_id = %descriptor.metadata.id,
            name = %descriptor.metadata.name,
            version = %descriptor.metadata.version,
            extension_type = ?extension_type,
            commands_count = descriptor.commands.len(),
            metrics_count = descriptor.metrics.len(),
            "Extension loaded successfully"
        );

        let runtime_handle = tokio::runtime::Handle::try_current()
            .map_err(|e| format!("Failed to get current runtime handle: {}", e))?;

        // Create IPC client for capability forwarding (both Native and WASM)
        let ipc_client = {
            let client = SyncIpcClient::new();
            Some(Arc::new(client))
        };

        if let (Some(extension), Some(ipc_client)) = (extension.as_ref(), ipc_client.as_ref()) {
            extension.install_capability_bridge(ipc_client.clone());
        }

        Ok(Self {
            extension,
            wasm_runtime,
            descriptor,
            extension_type,
            runtime: runtime_handle,
            running: true,
            ipc_client,
        })
    }

    /// Load a native extension and return its descriptor
    pub(crate) async fn load_native(
        extension_path: &Path,
    ) -> Result<(NativeExtensionBridge, ExtensionDescriptor), String> {
        debug!("load_native called");

        // Pre-load validation: check dylib headers before loading
        // This catches issues like incorrect LC_ID_DYLIB on macOS
        if let Err(e) = validate_library(extension_path) {
            error!(
                path = %extension_path.display(),
                error = %e,
                "Library validation failed - this extension may crash on other machines"
            );
            return Err(format!(
                "Library validation failed: {}. The extension may have been built incorrectly. \
                 On macOS, ensure LC_ID_DYLIB is set to '@rpath/extension.dylib' using: \
                 install_name_tool -id '@rpath/extension.dylib' extension.dylib",
                e
            ));
        }

        // Use catch_unwind to catch panics during library loading
        // This prevents the runner process from crashing abruptly
        let path = extension_path.to_path_buf();
        let load_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            NativeExtensionBridge::load(&path)
        }));

        match load_result {
            Ok(Ok((bridge, descriptor))) => {
                info!(
                    "Descriptor created: id='{}', commands={}, metrics={}",
                    descriptor.metadata.id,
                    descriptor.commands.len(),
                    descriptor.metrics.len()
                );
                Ok((bridge, descriptor))
            }
            Ok(Err(e)) => {
                // Extension returned an error (not a panic)
                Err(e)
            }
            Err(panic_payload) => {
                // Library loading panicked - extract panic message
                let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic during library loading".to_string()
                };
                error!(
                    path = %extension_path.display(),
                    panic = %panic_msg,
                    "Native library loading panicked - extension is likely corrupted or incompatible"
                );
                Err(format!(
                    "Library loading panicked: {}. The extension binary may be corrupted, \
                     incompatible with this platform, or built with incorrect settings.",
                    panic_msg
                ))
            }
        }
    }

    /// Load a WASM extension with full descriptor support
    pub(crate) async fn load_wasm(
        extension_path: &PathBuf,
    ) -> Result<(WasmRuntime, ExtensionDescriptor), String> {
        // First, create the runtime
        let module_name = extension_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let runtime = WasmRuntime::new(extension_path, module_name)?;

        // Try to get descriptor from WASM module itself
        match runtime.get_descriptor_blocking() {
            Ok(descriptor) => {
                debug!(
                    extension_id = %descriptor.metadata.id,
                    name = %descriptor.metadata.name,
                    version = %descriptor.metadata.version,
                    commands_count = descriptor.commands.len(),
                    metrics_count = descriptor.metrics.len(),
                    "Got descriptor from WASM module"
                );
                Ok((runtime, descriptor))
            }
            Err(e) => {
                debug!(error = %e, "Failed to get descriptor from WASM, trying sidecar files");

                // Fallback to sidecar JSON files
                let metadata = Self::load_wasm_metadata(extension_path)?;
                let descriptor = ExtensionDescriptor::new(metadata);
                Ok((runtime, descriptor))
            }
        }
    }

    /// Load WASM metadata (fallback from sidecar files)
    pub(crate) fn load_wasm_metadata(extension_path: &Path) -> Result<ExtensionMetadata, String> {
        // Try sidecar JSON
        let json_path = extension_path.with_extension("json");
        if json_path.exists() {
            if let Ok(meta) = Self::parse_metadata_json(&json_path) {
                return Ok(meta);
            }
        }

        // Try .nep manifest
        if let Some(manifest_path) = Self::find_nep_manifest(extension_path) {
            if manifest_path.exists() {
                if let Ok(meta) = Self::parse_metadata_json(&manifest_path) {
                    return Ok(meta);
                }
            }
        }

        // Fallback to filename
        let file_name = extension_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        Ok(ExtensionMetadata::new(
            file_name.to_string(),
            format!("{} WASM Extension", file_name),
            "1.0.0",
        ))
    }

    pub(crate) fn parse_metadata_json(path: &PathBuf) -> Result<ExtensionMetadata, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read JSON: {}", e))?;

        #[derive(serde::Deserialize)]
        struct MetaJson {
            id: String,
            name: String,
            version: String,
            #[serde(default)]
            description: Option<String>,
            #[serde(default)]
            author: Option<String>,
        }

        let json: MetaJson =
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        let version =
            semver::Version::parse(&json.version).unwrap_or(semver::Version::new(1, 0, 0));

        let mut meta = ExtensionMetadata::new(json.id, json.name, version.to_string());
        meta.description = json.description;
        meta.author = json.author;

        Ok(meta)
    }

    pub(crate) fn find_nep_manifest(wasm_path: &Path) -> Option<PathBuf> {
        let binaries_dir = wasm_path.parent()?;
        let wasm_dir = binaries_dir.parent()?;
        let extension_folder = wasm_dir.parent()?;

        let manifest = extension_folder.join("manifest.json");
        if manifest.exists() {
            return Some(manifest);
        }
        None
    }

    /// Run the main loop
    pub(crate) async fn run(&mut self) {
        debug!("Starting IPC message loop");

        // Note: We no longer send Ready here - we wait for Init message first
        // This ensures proper handshake sequence: host sends Init, we respond with Ready

        debug!("Waiting for Init message from host");

        // Create event channel BEFORE starting stdin reader to prevent race condition.
        // If stdin reader starts first, it may read the Init message before EVENT_TX
        // is initialized, causing the message to be silently dropped and a 120s timeout.
        let mut event_rx = create_event_channel();

        // Start stdin reader thread (reads all stdin messages and routes them)
        let _stdin_reader_handle = start_stdin_reader();

        // Start push-output stdout writer thread (drains buffer → stdout)
        PUSH_BUFFER
            .set((
                std::sync::Mutex::new(VecDeque::with_capacity(PUSH_BUFFER_CAPACITY)),
                std::sync::Condvar::new(),
            ))
            .expect("PUSH_BUFFER already initialized");
        std::thread::Builder::new()
            .name("push-stdout-writer".into())
            .spawn(push_stdout_writer_thread)
            .expect("Failed to spawn push stdout writer thread");
        debug!("Push stdout writer thread started");
        debug!("Stdin reader thread started");

        while self.running {
            // Channel recv blocks efficiently - zero CPU when idle
            tokio::select! {
                Some(message) = event_rx.recv() => {
                    debug!(
                        "Received IPC message from channel: {:?}",
                        std::mem::discriminant(&message)
                    );
                    self.handle_message(message).await;
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                    // Periodic keepalive check
                    trace!("Extension runner keepalive");
                }
            }
        }

        debug!("Extension runner shutting down");
    }

    /// Start the IPC capability forwarder thread
    ///
    pub(crate) fn send_response(&mut self, response: IpcResponse) {
        debug!(response_type = ?std::mem::discriminant(&response), "Sending IPC response");

        // Debug: log StreamSessionInit responses specifically
        if let IpcResponse::StreamSessionInit {
            request_id,
            session_id,
            success,
            ..
        } = &response
        {
            debug!(
                request_id,
                session_id, success, "Sending StreamSessionInit response"
            );
        }

        let payload = match response.to_bytes() {
            Ok(p) => {
                debug!(payload_len = p.len(), "Response serialized successfully");
                p
            }
            Err(e) => {
                error!(error = %e, "Failed to serialize response");
                return;
            }
        };

        let frame = IpcFrame::new(payload);
        let bytes = frame.encode();

        debug!(frame_len = bytes.len(), "Frame encoded");

        let _guard = STDOUT_WRITE_MUTEX.lock().unwrap_or_else(|e| {
            error!("STDOUT_WRITE_MUTEX poisoned: {}", e);
            e.into_inner()
        });
        if let Err(e) = std::io::stdout().write_all(&bytes) {
            error!(error = %e, "Failed to write response");
            return;
        }
        if let Err(e) = std::io::stdout().flush() {
            error!(error = %e, "Failed to flush stdout");
        }

        // Debug: confirm StreamSessionInit was sent
        if let IpcResponse::StreamSessionInit {
            request_id,
            session_id,
            ..
        } = &response
        {
            debug!(request_id, session_id, "StreamSessionInit response sent");
        }
    }

    /// Send a capability request to the host and wait for response
    /// This is used for bidirectional IPC communication
    pub(crate) fn invoke_host_capability(
        &mut self,
        capability: &str,
        params: &serde_json::Value,
    ) -> serde_json::Value {
        use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

        static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
        let request_id = REQUEST_ID_COUNTER.fetch_add(1, AtomicOrdering::SeqCst);

        debug!(
            request_id,
            capability = %capability,
            "Sending CapabilityRequest to host"
        );

        // Send CapabilityRequest
        let request = IpcResponse::CapabilityRequest {
            request_id,
            capability: capability.to_string(),
            params: params.clone(),
        };

        let payload = match request.to_bytes() {
            Ok(p) => p,
            Err(e) => {
                error!(error = %e, "Failed to serialize CapabilityRequest");
                return json!({"success": false, "error": "Failed to serialize request"});
            }
        };

        let frame = IpcFrame::new(payload);
        let bytes = frame.encode();

        {
            let _guard = STDOUT_WRITE_MUTEX.lock().unwrap_or_else(|e| {
                error!("STDOUT_WRITE_MUTEX poisoned: {}", e);
                e.into_inner()
            });
            if let Err(e) = std::io::stdout().write_all(&bytes) {
                drop(_guard);
                error!(error = %e, "Failed to write CapabilityRequest");
                return json!({"success": false, "error": "Failed to write request"});
            }
            if let Err(e) = std::io::stdout().flush() {
                drop(_guard);
                error!(error = %e, "Failed to flush CapabilityRequest");
                return json!({"success": false, "error": "Failed to flush request"});
            }
        }

        debug!(request_id, "CapabilityRequest sent, waiting for response");

        // Read response from host
        // Read length prefix (4 bytes)
        let mut len_bytes = [0u8; 4];
        match std::io::stdin().read_exact(&mut len_bytes) {
            Ok(_) => {}
            Err(e) => {
                error!(error = %e, "Failed to read response length");
                return json!({"success": false, "error": "Failed to read response length"});
            }
        }

        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > 10 * 1024 * 1024 {
            error!(len, "Response too large");
            return json!({"success": false, "error": "Response too large"});
        }

        // Read payload
        let mut payload_buf = vec![0u8; len];
        match std::io::stdin().read_exact(&mut payload_buf) {
            Ok(_) => {}
            Err(e) => {
                error!(error = %e, "Failed to read response payload");
                return json!({"success": false, "error": "Failed to read response payload"});
            }
        }

        // Parse response
        let response: IpcResponse = match IpcResponse::from_bytes(&payload_buf) {
            Ok(r) => r,
            Err(e) => {
                error!(error = %e, "Failed to parse response");
                return json!({"success": false, "error": "Failed to parse response"});
            }
        };

        match response {
            IpcResponse::CapabilityResult {
                request_id: resp_id,
                result,
                error,
            } => {
                if resp_id != request_id {
                    warn!(
                        expected = request_id,
                        got = resp_id,
                        "Request ID mismatch in CapabilityResult"
                    );
                }
                if let Some(err) = error {
                    json!({"success": false, "error": err})
                } else {
                    result
                }
            }
            _ => {
                error!("Unexpected response type to CapabilityRequest");
                json!({"success": false, "error": "Unexpected response type"})
            }
        }
    }

    pub(crate) async fn handle_message(&mut self, message: IpcMessage) {
        match message {
            IpcMessage::Init { config } => {
                debug!("Received Init message from host with config");

                if let Err(error) = self.handle_init(config).await {
                    self.send_response(IpcResponse::Error {
                        request_id: 0,
                        error,
                        kind: ErrorKind::Internal,
                    });
                    return;
                }

                // Debug: Log descriptor details before sending
                debug!(
                    descriptor_id = %self.descriptor.id(),
                    commands_count = self.descriptor.commands.len(),
                    metrics_count = self.descriptor.metrics.len(),
                    "Sending Ready response with descriptor"
                );

                // Debug: Log each command and metric
                for (i, cmd) in self.descriptor.commands.iter().enumerate() {
                    debug!(command_index = i, command_name = %cmd.name, "Command {}", i);
                }
                for (i, metric) in self.descriptor.metrics.iter().enumerate() {
                    debug!(metric_index = i, metric_name = %metric.name, "Metric {}", i);
                }

                // Try to serialize descriptor to verify it's valid
                match serde_json::to_string(&self.descriptor) {
                    Ok(json_str) => {
                        debug!(
                            json_len = json_str.len(),
                            "Descriptor serialized successfully"
                        );
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to serialize descriptor");
                    }
                }

                self.send_response(IpcResponse::Ready {
                    descriptor: self.descriptor.clone(),
                });

                debug!("Ready response sent to host");
            }

            IpcMessage::ExecuteCommand {
                command,
                args,
                request_id,
            } => {
                self.handle_execute_command(command, args, request_id).await;
            }

            IpcMessage::ProduceMetrics { request_id } => {
                self.handle_produce_metrics(request_id).await;
            }

            IpcMessage::GetDescriptor { request_id } => {
                self.handle_get_descriptor(request_id).await;
            }

            IpcMessage::HealthCheck { request_id } => {
                self.handle_health_check(request_id);
            }

            IpcMessage::GetMetadata { request_id } => {
                self.send_response(IpcResponse::Metadata {
                    request_id,
                    metadata: self.descriptor.metadata.clone(),
                });
            }

            IpcMessage::GetEventSubscriptions { request_id } => {
                // Get event subscriptions from the extension
                let event_types = if let Some(extension) = &self.extension {
                    extension.event_subscriptions()
                } else {
                    vec![]
                };

                self.send_response(IpcResponse::EventSubscriptions {
                    request_id,
                    event_types,
                });
            }

            IpcMessage::GetStats { request_id } => {
                self.handle_get_stats(request_id);
            }

            IpcMessage::Shutdown => {
                debug!("Received shutdown command");
                self.send_response(IpcResponse::ShutdownAck);
                self.running = false;
            }

            IpcMessage::ConfigUpdate { config } => {
                debug!("Received config hot-reload update");
                let result = match self.extension.as_ref() {
                    Some(ext) => match ext.configure(&config) {
                        Ok(()) => {
                            debug!("Config hot-reload applied successfully");
                            IpcResponse::ConfigUpdated {
                                success: true,
                                error: None,
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "Config hot-reload failed");
                            IpcResponse::ConfigUpdated {
                                success: false,
                                error: Some(e.to_string()),
                            }
                        }
                    },
                    None => IpcResponse::ConfigUpdated {
                        success: false,
                        error: Some("Extension not loaded".to_string()),
                    },
                };
                self.send_response(result);
            }

            IpcMessage::Ping {
                request_id,
                timestamp,
            } => {
                self.send_response(IpcResponse::Pong {
                    request_id,
                    timestamp,
                });
            }

            // Streaming support
            IpcMessage::GetStreamCapability { request_id } => {
                self.handle_get_stream_capability(request_id).await;
            }

            IpcMessage::InitStreamSession {
                request_id,
                session_id,
                extension_id: _,
                config,
                client_info: _,
            } => {
                self.handle_init_stream_session(request_id, session_id, config)
                    .await;
            }

            IpcMessage::ProcessStreamChunk {
                request_id,
                session_id,
                chunk,
            } => {
                self.handle_process_stream_chunk(request_id, session_id, chunk);
            }

            IpcMessage::CloseStreamSession {
                request_id,
                session_id,
            } => {
                self.handle_close_stream_session(request_id, session_id);
            }

            // Stateless mode support
            IpcMessage::ProcessChunk { request_id, chunk } => {
                self.handle_process_chunk(request_id, chunk);
            }

            // Push mode support
            IpcMessage::StartPush {
                request_id,
                session_id,
            } => {
                self.handle_start_push(request_id, session_id);
            }

            IpcMessage::StopPush {
                request_id,
                session_id,
            } => {
                self.handle_stop_push(request_id, session_id);
            }

            // Batch command support
            IpcMessage::ExecuteBatch {
                commands,
                request_id,
            } => {
                self.handle_execute_batch(commands, request_id).await;
            }

            // Capability invocation support (for WASM extensions)
            IpcMessage::InvokeCapability {
                request_id,
                capability,
                params,
            } => {
                self.handle_invoke_capability(request_id, capability, params);
            }

            IpcMessage::SubscribeEvents {
                request_id,
                event_types: _,
                filter: _,
            } => {
                // Generate subscription ID
                let subscription_id = uuid::Uuid::new_v4().to_string();
                self.send_response(IpcResponse::EventSubscriptionResult {
                    request_id,
                    subscription_id: Some(subscription_id),
                    error: None,
                });
            }

            IpcMessage::UnsubscribeEvents {
                request_id,
                subscription_id: _,
            } => {
                self.send_response(IpcResponse::EventSubscriptionResult {
                    request_id,
                    subscription_id: None,
                    error: None,
                });
            }

            IpcMessage::PollEvents {
                request_id,
                subscription_id: _,
            } => {
                // Return empty events for now
                self.send_response(IpcResponse::EventPollResult {
                    request_id,
                    events: vec![],
                });
            }

            IpcMessage::EventPush {
                event_type,
                payload,
                timestamp: _,
            } => {
                // Handle event push from host
                debug!(
                    event_type = %event_type,
                    "Received event push from host"
                );

                // Forward event pushes to the native JSON bridge when available.
                if let Some(extension) = &self.extension {
                    match extension.handle_event(&event_type, &payload) {
                        Ok(_) => {
                            trace!(
                                event_type = %event_type,
                                "Event handled by extension"
                            );
                        }
                        Err(e) => {
                            error!(
                                event_type = %event_type,
                                error = %e,
                                "Failed to handle event in extension"
                            );
                        }
                    }
                } else {
                    // Call the SDK's event handler for native extensions (fallback)
                    neomind_extension_sdk::capabilities::event::call_event_handler(
                        &event_type,
                        &payload,
                    );
                }

                // For WASM extensions, push events to global subscription queues
                if self.extension_type == ExtensionType::Wasm {
                    get_global_event_state().push_event(&event_type, payload);
                }
            }

            IpcMessage::CapabilityResult {
                request_id,
                result,
                error,
            } => {
                // Handle capability result from host (response to our CapabilityRequest)
                debug!(
                    request_id,
                    has_error = error.is_some(),
                    "Received CapabilityResult from host"
                );

                // Route to waiting invoke() call via pending requests queue
                let response = IpcResponse::CapabilityResult {
                    request_id,
                    result,
                    error,
                };
                complete_pending_request(request_id, response);
            }
        }
    }

    pub(crate) async fn handle_execute_command(
        &mut self,
        command: String,
        args: serde_json::Value,
        request_id: u64,
    ) {
        debug!(command = %command, request_id, "Executing command");

        let result = match self.extension_type {
            ExtensionType::Native => self.execute_native_command(&command, &args).await,
            ExtensionType::Wasm => self.execute_wasm_command(&command, &args),
        };

        match result {
            Ok(value) => {
                self.send_response(IpcResponse::Success {
                    request_id,
                    data: value,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e.clone(),
                    kind: ErrorKind::ExecutionFailed,
                });
            }
        }
    }

    pub(crate) async fn handle_init(&mut self, config: serde_json::Value) -> Result<(), String> {
        match self.extension_type {
            ExtensionType::Native => {
                let ext = self
                    .extension
                    .as_ref()
                    .ok_or("No native extension loaded")?;
                ext.configure(&config)
            }
            ExtensionType::Wasm => Ok(()),
        }
    }

    pub(crate) async fn handle_execute_batch(
        &mut self,
        commands: Vec<BatchCommand>,
        request_id: u64,
    ) {
        debug!(
            request_id,
            command_count = commands.len(),
            "Executing batch command"
        );

        let start = std::time::Instant::now();
        let mut results = Vec::new();

        for cmd in &commands {
            let cmd_start = std::time::Instant::now();

            let result = match self.extension_type {
                ExtensionType::Native => self.execute_native_command(&cmd.command, &cmd.args).await,
                ExtensionType::Wasm => self.execute_wasm_command(&cmd.command, &cmd.args),
            };

            results.push(BatchResult {
                command: cmd.command.clone(),
                success: result.is_ok(),
                data: result.as_ref().ok().cloned(),
                error: result.as_ref().err().map(|e| e.to_string()),
                elapsed_ms: cmd_start.elapsed().as_millis() as f64,
            });
        }

        self.send_response(IpcResponse::BatchResults {
            request_id,
            results,
            total_elapsed_ms: start.elapsed().as_millis() as f64,
        });
    }

    pub(crate) async fn execute_native_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.execute_command(command, args)
    }

    pub(crate) fn execute_wasm_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let runtime = self.wasm_runtime.as_ref().ok_or("No WASM runtime loaded")?;

        let ipc_client = self.ipc_client.clone();

        let runtime_handle = self.runtime.clone();
        tokio::task::block_in_place(|| {
            runtime_handle.block_on(async {
                // Try new execute_command API first
                match runtime
                    .execute_command(command, args, ipc_client.clone())
                    .await
                {
                    Ok(result) => {
                        // Extract the actual result from the response
                        if result
                            .get("success")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            Ok(result.get("result").cloned().unwrap_or(result))
                        } else {
                            Err(result
                                .get("error")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error")
                                .to_string())
                        }
                    }
                    Err(_) => {
                        // Fallback to legacy execute function
                        runtime.execute(command, args, ipc_client.clone()).await
                    }
                }
            })
        })
    }

    pub(crate) async fn handle_produce_metrics(&mut self, request_id: u64) {
        debug!(request_id, "Producing metrics");

        let result = match self.extension_type {
            ExtensionType::Native => self.produce_native_metrics().await,
            ExtensionType::Wasm => self.produce_wasm_metrics(),
        };

        match result {
            Ok(metrics) => {
                self.send_response(IpcResponse::Metrics {
                    request_id,
                    metrics,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::Internal,
                });
            }
        }
    }

    pub(crate) async fn produce_native_metrics(&self) -> Result<Vec<ExtensionMetricValue>, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.produce_metrics()
    }

    pub(crate) fn produce_wasm_metrics(&self) -> Result<Vec<ExtensionMetricValue>, String> {
        let runtime = self.wasm_runtime.as_ref().ok_or("No WASM runtime loaded")?;
        runtime.produce_metrics()
    }

    /// Build a fresh descriptor by asking the extension (native FFI or WASM)
    /// to regenerate metadata/commands/metrics. The returned descriptor
    /// reflects the extension's current runtime state, enabling dynamic
    /// metrics discovery.
    pub(crate) fn build_descriptor_fresh(&self) -> Result<ExtensionDescriptor, String> {
        match self.extension_type {
            ExtensionType::Native => {
                let ext = self
                    .extension
                    .as_ref()
                    .ok_or("No native extension loaded")?;
                ext.get_descriptor_fresh()
            }
            ExtensionType::Wasm => {
                let runtime = self.wasm_runtime.as_ref().ok_or("No WASM runtime loaded")?;
                runtime.get_descriptor_blocking()
            }
        }
    }

    /// Handler for `IpcMessage::GetDescriptor`. Rebuilds the descriptor and
    /// updates the runner's cached copy so subsequent `GetMetadata` and
    /// `Ready` responses also reflect the latest state.
    pub(crate) async fn handle_get_descriptor(&mut self, request_id: u64) {
        debug!(request_id, "Refreshing descriptor");
        match self.build_descriptor_fresh() {
            Ok(descriptor) => {
                let metrics_count = descriptor.metrics.len();
                let commands_count = descriptor.commands.len();
                self.descriptor = descriptor.clone();
                debug!(
                    request_id,
                    metrics_count, commands_count, "Descriptor refreshed"
                );
                self.send_response(IpcResponse::Descriptor {
                    request_id,
                    descriptor,
                });
            }
            Err(e) => {
                warn!(request_id, error = %e, "Failed to refresh descriptor");
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::Internal,
                });
            }
        }
    }

    pub(crate) fn handle_health_check(&mut self, request_id: u64) {
        debug!(request_id, "Health check");

        let healthy = match self.extension_type {
            ExtensionType::Native => self.native_health_check(),
            ExtensionType::Wasm => self.wasm_health_check(),
        };

        self.send_response(IpcResponse::Health {
            request_id,
            healthy,
        });
    }

    pub(crate) fn handle_invoke_capability(
        &mut self,
        request_id: u64,
        capability: String,
        params: serde_json::Value,
    ) {
        debug!(request_id, capability = %capability, "Invoking capability");

        // Forward the capability request to the host via bidirectional IPC
        // The host will invoke the real capability provider and return the result
        let result = self.invoke_host_capability(&capability, &params);

        // Extract error from result if present
        let (result_value, error) = if let Some(err) = result.get("error").and_then(|e| e.as_str())
        {
            (serde_json::json!({}), Some(err.to_string()))
        } else {
            (result, None)
        };

        self.send_response(IpcResponse::CapabilityResult {
            request_id,
            result: result_value,
            error,
        });
    }

    pub(crate) fn native_health_check(&self) -> bool {
        let ext = match &self.extension {
            Some(e) => e,
            None => return false,
        };
        ext.health_check()
    }

    pub(crate) fn wasm_health_check(&self) -> bool {
        let runtime = match &self.wasm_runtime {
            Some(r) => r,
            None => return false,
        };

        let runtime_handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return false,
        };
        tokio::task::block_in_place(|| {
            runtime_handle.block_on(async { runtime.health_check().await })
        })
    }

    // =========================================================================
    // Statistics Support
    // =========================================================================

    pub(crate) fn handle_get_stats(&mut self, request_id: u64) {
        debug!(request_id, "Getting extension statistics");

        let stats = match self.extension_type {
            ExtensionType::Native => self.get_native_stats(),
            ExtensionType::Wasm => {
                // WASM extensions don't support stats yet
                // Return default stats
                ExtensionStats::default()
            }
        };

        debug!(
            request_id,
            start_count = stats.start_count,
            stop_count = stats.stop_count,
            error_count = stats.error_count,
            "Sending Stats response"
        );

        self.send_response(IpcResponse::Stats {
            request_id,
            start_count: stats.start_count,
            stop_count: stats.stop_count,
            error_count: stats.error_count,
            last_error: stats.last_error,
        });

        debug!(request_id, "Stats response sent");
    }

    pub(crate) fn get_native_stats(&self) -> ExtensionStats {
        debug!("Getting native extension stats");
        let ext = match &self.extension {
            Some(e) => e,
            None => {
                debug!("No extension loaded, returning default stats");
                return ExtensionStats::default();
            }
        };
        let stats = ext.get_stats();
        debug!(
            start_count = stats.start_count,
            stop_count = stats.stop_count,
            error_count = stats.error_count,
            "Got extension stats"
        );
        stats
    }

    // =========================================================================
    // Streaming Support
    // =========================================================================

    pub(crate) async fn handle_get_stream_capability(&mut self, request_id: u64) {
        debug!(request_id, "Getting stream capability");

        let capability = match self.extension_type {
            ExtensionType::Native => self.get_native_stream_capability().await,
            ExtensionType::Wasm => {
                Ok(None) // WASM doesn't support streaming yet
            }
        };

        match capability {
            Ok(cap) => {
                self.send_response(IpcResponse::StreamCapability {
                    request_id,
                    capability: cap.map(|c| serde_json::to_value(c).unwrap_or_default()),
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::Internal,
                });
            }
        }
    }

    pub(crate) async fn get_native_stream_capability(
        &self,
    ) -> Result<Option<StreamCapability>, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.get_stream_capability()
    }

    pub(crate) async fn handle_init_stream_session(
        &mut self,
        request_id: u64,
        session_id: String,
        config: serde_json::Value,
    ) {
        debug!(session_id = %session_id, request_id, "Initializing stream session");

        let result = match self.extension_type {
            ExtensionType::Native => self.init_native_stream_session(&session_id, config).await,
            ExtensionType::Wasm => Err("WASM streaming not supported".to_string()),
        };

        match result {
            Ok(_) => {
                self.send_response(IpcResponse::StreamSessionInit {
                    request_id,
                    session_id,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::StreamSessionInit {
                    request_id,
                    session_id,
                    success: false,
                    error: Some(e),
                });
            }
        }
    }
    pub(crate) async fn init_native_stream_session(
        &self,
        session_id: &str,
        config: serde_json::Value,
    ) -> Result<(), String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.init_session(session_id, config)
    }
    pub(crate) fn handle_process_stream_chunk(
        &mut self,
        request_id: u64,
        session_id: String,
        chunk: StreamDataChunk,
    ) {
        debug!(session_id = %session_id, sequence = chunk.sequence, request_id, "Processing stream chunk");

        let result = match self.extension_type {
            ExtensionType::Native => self.process_native_stream_chunk(&session_id, chunk),
            ExtensionType::Wasm => Err("WASM streaming not supported".to_string()),
        };

        match result {
            Ok(stream_result) => {
                self.send_response(IpcResponse::StreamChunkResult {
                    request_id,
                    session_id,
                    input_sequence: stream_result.input_sequence.unwrap_or(0),
                    output_sequence: stream_result.output_sequence,
                    data: stream_result.data,
                    data_type: stream_result.data_type.mime_type(),
                    processing_ms: stream_result.processing_ms,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::ExecutionFailed,
                });
            }
        }
    }

    pub(crate) fn process_native_stream_chunk(
        &self,
        session_id: &str,
        chunk: StreamDataChunk,
    ) -> Result<StreamResult, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.process_session_chunk(session_id, chunk)
    }

    pub(crate) fn handle_close_stream_session(&mut self, request_id: u64, session_id: String) {
        debug!(session_id = %session_id, request_id, "Closing stream session");

        let result = match self.extension_type {
            ExtensionType::Native => self.close_native_stream_session(&session_id),
            ExtensionType::Wasm => Ok(SessionStats::default()),
        };

        match result {
            Ok(stats) => {
                self.send_response(IpcResponse::StreamSessionClosed {
                    request_id,
                    session_id,
                    total_frames: stats.input_chunks,
                    duration_ms: 0, // We don't track this in runner
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::ExecutionFailed,
                });
            }
        }
    }

    pub(crate) fn close_native_stream_session(
        &self,
        session_id: &str,
    ) -> Result<SessionStats, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.close_session(session_id)
    }

    // =========================================================================
    // Stateless Mode Support
    // =========================================================================

    pub(crate) fn handle_process_chunk(&mut self, request_id: u64, chunk: StreamDataChunk) {
        debug!(
            request_id,
            sequence = chunk.sequence,
            "Processing stateless chunk"
        );

        let result = match self.extension_type {
            ExtensionType::Native => self.process_native_chunk(chunk),
            ExtensionType::Wasm => {
                Err("Stateless chunk processing not supported for WASM".to_string())
            }
        };

        match result {
            Ok(stream_result) => {
                self.send_response(IpcResponse::ChunkResult {
                    request_id,
                    input_sequence: stream_result.input_sequence.unwrap_or(0),
                    output_sequence: stream_result.output_sequence,
                    data: stream_result.data,
                    data_type: stream_result.data_type.mime_type(),
                    processing_ms: stream_result.processing_ms,
                    metadata: stream_result.metadata,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::Error {
                    request_id,
                    error: e,
                    kind: ErrorKind::ExecutionFailed,
                });
            }
        }
    }

    pub(crate) fn process_native_chunk(
        &self,
        chunk: StreamDataChunk,
    ) -> Result<StreamResult, String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.process_chunk(chunk)
    }

    // =========================================================================
    // Push Mode Support
    // =========================================================================

    pub(crate) fn handle_start_push(&mut self, request_id: u64, session_id: String) {
        debug!(
            request_id,
            session_id = %session_id,
            "Starting push mode"
        );

        let result = match self.extension_type {
            ExtensionType::Native => self.start_native_push(&session_id),
            ExtensionType::Wasm => Err("Push mode not supported for WASM".to_string()),
        };

        match result {
            Ok(_) => {
                self.send_response(IpcResponse::PushStarted {
                    request_id,
                    session_id,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                self.send_response(IpcResponse::PushStarted {
                    request_id,
                    session_id,
                    success: false,
                    error: Some(e),
                });
            }
        }
    }

    pub(crate) fn start_native_push(&self, session_id: &str) -> Result<(), String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.start_push(session_id)
    }

    pub(crate) fn handle_stop_push(&mut self, request_id: u64, session_id: String) {
        debug!(
            request_id,
            session_id = %session_id,
            "Stopping push mode"
        );

        let result = match self.extension_type {
            ExtensionType::Native => self.stop_native_push(&session_id),
            ExtensionType::Wasm => {
                Ok(()) // WASM doesn't support push, just return success
            }
        };

        match result {
            Ok(_) => {
                self.send_response(IpcResponse::PushStopped {
                    request_id,
                    session_id,
                    success: true,
                });
            }
            Err(e) => {
                warn!(
                    session_id = %session_id,
                    error = %e,
                    "Error stopping push mode"
                );
                self.send_response(IpcResponse::PushStopped {
                    request_id,
                    session_id,
                    success: false,
                });
            }
        }
    }

    pub(crate) fn stop_native_push(&self, session_id: &str) -> Result<(), String> {
        let ext = self
            .extension
            .as_ref()
            .ok_or("No native extension loaded")?;
        ext.stop_push(session_id)
    }
}

// Worker-thread count: configurable via NEOMIND_RUNNER_WORKERS, default 4
// — DO NOT lower the default. The runtime hosts command dispatch (FFI
// calls into extensions BLOCK their worker) and async stream sessions
// (e.g. bidirectional audio); 2 workers can stall health checks/IPC
// routing under two concurrent blocking commands. The knob exists for
// memory-constrained hosts (NE503 on-device) where the tradeoff is
// deliberate.
pub(crate) fn runner_worker_threads() -> usize {
    std::env::var("NEOMIND_RUNNER_WORKERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| (1..=16).contains(n))
        .unwrap_or(4)
}
