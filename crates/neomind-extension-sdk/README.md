# NeoMind Extension SDK

**Version**: 0.7.1 | **ABI version**: 3 | **MSRV**: 1.75 | **License**: MIT OR Apache-2.0

Unified SDK for developing NeoMind Edge AI Platform extensions — a single codebase
that compiles to both **Native** (dynamic library, loaded by the extension runner)
and **WASM** (executed via wasmtime) targets.

## Highlights

- **Unified SDK** — Native and WASM from one codebase; target-specific details are `#[cfg]`-gated
- **One-line FFI export** — `neomind_export!(MyExtension)` generates the whole FFI surface
- **Process isolation** — every extension runs in its own runner process; crashes never take down the NeoMind core
- **Capability system** — 20 built-in capabilities (devices, events, telemetry, agents, chat, rules) plus custom capabilities
- **Streaming & push mode** — pull/stateless/stateful sessions and continuous push output
- **Zero-serialization push** (0.7) — the raw FFI writer moves binary payloads (video access units, 35–300 KB) from your `Vec<u8>` to the IPC segment **without JSON serialization or base64**; only the small metadata is JSON-encoded
- **Segmented payload codec** (0.7) — `encode_segmented_payload` / `parse_response_payload` eliminate base64 on the runner→core leg too
- **Test kit** — protocol-level testing (in-process mock runner, capability recorder, event injector, timing assertions) behind the `testkit` feature

## Architecture

All extensions run in isolated processes:

```text
┌─────────────────────────────────────────────────────────────┐
│                   NeoMind Main Process                       │
│  - UnifiedExtensionService manages all extensions           │
│  - IPC communication via stdin/stdout                        │
└─────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                  Extension Runner Process                    │
│  - Your extension runs here in isolation                    │
│  - Native: loaded via FFI                                   │
│  - WASM: executed via wasmtime                              │
│  - Crashes don't affect the main process                    │
└─────────────────────────────────────────────────────────────┘
```

## Quick start

Add the SDK to your extension's `Cargo.toml`:

```toml
[dependencies]
neomind-extension-sdk = "0.7"

[lib]
crate-type = ["cdylib"]

[profile.release]
panic = "unwind"   # required — the runner catches panics at the FFI boundary
opt-level = 3
lto = "thin"
```

Minimum example:

```rust
use neomind_extension_sdk::prelude::*;
use std::sync::atomic::{AtomicI64, Ordering};

pub struct MyExtension {
    counter: AtomicI64,
}

#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static_metadata!("my-extension", "My Extension", "1.0.0")
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricBuilder::new("counter", "Counter")
                .integer()
                .unit("count")
                .build(),
        ]
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![
            CommandBuilder::new("increment")
                .display_name("Increment")
                .param(
                    ParamBuilder::new("amount", MetricDataType::Integer)
                        .display_name("Amount")
                        .default(MetricValue::Integer(1))
                        .build(),
                )
                .build(),
        ]
    }

    async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
        match cmd {
            "increment" => {
                let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(1);
                let new_value = self.counter.fetch_add(amount, Ordering::SeqCst) + amount;
                Ok(json!({ "counter": new_value }))
            }
            _ => Err(ExtensionError::CommandNotFound(cmd.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![ExtensionMetricValue::new(
            "counter",
            MetricValue::Integer(self.counter.load(Ordering::SeqCst)),
        )])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Export the FFI surface — this one line is all it takes.
neomind_extension_sdk::neomind_export!(MyExtension);
```

## The `Extension` trait

| Method | Required | Purpose |
|--------|----------|---------|
| `metadata()` | ✅ | Static identity: id, name, version, description, author |
| `as_any()` | ✅ | Downcasting support |
| `execute_command()` | default: `CommandNotFound` | Handle commands from the platform/CLI/UI |
| `produce_metrics()` | optional | Emit current metric values |
| `metrics()` / `commands()` | optional | Declare metric and command descriptors |
| `init()` / `start()` / `stop()` / `on_unload()` | optional | Lifecycle hooks |
| `configure()` | optional | Receive configuration updates |
| `health_check()` | optional | Liveness probe |
| `event_subscriptions()` / `handle_event()` | optional | Subscribe to and handle platform events |
| `init_session()` / `process_session_chunk()` / `close_session()` | optional | Stateful streaming sessions |
| `process_chunk()` | optional | Stateless streaming |
| `start_push()` / `stop_push()` / `latest_output()` | optional | Push mode (continuous output) |
| `stream_capability()` | optional | Advertise direction/mode/limits |
| `descriptor()` / `status()` / `get_stats()` | optional | Richer descriptors for the dashboard |

## Macros

```rust
// FFI export (generates all entry points, including the optional
// register_push_writer_raw export that new runners resolve)
neomind_extension_sdk::neomind_export!(MyExtension);

// Static metadata / metrics / commands without OnceLock boilerplate
static_metadata!("my-extension", "My Extension", "1.0.0");
static_metrics![..];
static_commands![..];
```

Helper macros: `metric_int!`, `metric_float!`, `metric_bool!`, `metric_string!`,
and logging via `ext_info!`, `ext_debug!`, `ext_warn!`, `ext_error!`.

## Builders

```rust
// Metric descriptor
let metric = MetricBuilder::new("temperature", "Temperature")
    .float()
    .unit("°C")
    .min(-50.0)
    .max(150.0)
    .required()
    .build();

// Command with typed parameters and a sample payload
let command = CommandBuilder::new("increment")
    .display_name("Increment")
    .description("Increment the counter")
    .param_simple("amount", "Amount", MetricDataType::Integer)
    .sample(json!({ "amount": 1 }))
    .build();

// Parameter definition
let param = ParamBuilder::new("amount", MetricDataType::Integer)
    .display_name("Amount")
    .description("Amount to add")
    .default(MetricValue::Integer(1))
    .min(1.0)
    .max(100.0)
    .build();
```

## Capability system

Extensions access NeoMind platform features through capabilities:

```rust
use neomind_extension_sdk::capabilities::{agent, device, event, rule};

// Read device metrics
let metrics = device::get_metrics(&context, "device-1").await?;

// Write a virtual metric
device::write_virtual_metric(&context, "device-1", "calculated_value", &json!(42.5)).await?;

// Send a device command
device::send_command(&context, "device-1", "set_level", &json!({"level": 80})).await?;

// Publish an event
event::publish(&context, event).await?;

// Trigger an agent
agent::trigger(&context, "analyzer-agent", &json!({"query": "analyze"})).await?;

// Trigger a rule
rule::trigger(&context, "alert-rule", &json!({"value": 85})).await?;
```

Built-in capabilities:

| Capability | Name | Description |
|------------|------|-------------|
| DeviceMetricsRead | `device_metrics_read` | Read device metrics (current state) |
| DeviceMetricsWrite | `device_metrics_write` | Write device metrics (incl. virtual metrics) |
| DeviceControl | `device_control` | Send device commands |
| StorageQuery | `storage_query` | Storage queries (read telemetry) |
| EventPublish | `event_publish` | Publish events |
| EventSubscribe | `event_subscribe` | Subscribe to events |
| TelemetryHistory | `telemetry_history` | Query device telemetry history |
| MetricsAggregate | `metrics_aggregate` | Aggregate device metrics |
| ExtensionCall | `extension_call` | Call other extensions |
| AgentTrigger | `agent_trigger` | Trigger agents |
| ChatStream | `chat_stream` | Streaming chat with token-level events |
| ChatStreamCancel | `chat_stream_cancel` | Cancel an in-flight chat stream |
| ChatSessionOpen | `chat_session_open` | Open a persistent chat session |
| ChatSessionSend | `chat_session_send` | Send a message to an open session |
| ChatSessionClose | `chat_session_close` | Close a chat session |
| ChatStreamCancelTurn | `chat_stream_cancel_turn` | Cancel the current turn only |
| RuleTrigger | `rule_trigger` | Trigger rules |
| DeviceTemplateRegister | `device_template_register` | Register device type templates |
| DeviceRegister | `device_register` | Register device instances |
| DeviceUnregister | `device_unregister` | Unregister device instances |

Unknown capability names map to `ExtensionCapability::Custom`, so hosts can
provide capabilities beyond the built-in set.

## Push mode & streaming

For continuous data (video relay, sensor streams) implement the push-mode hooks
(`start_push` / `stop_push` / `latest_output`) and call `send_push_output`:

```rust
use neomind_extension_sdk::{send_push_output, PushOutputMessage};

send_push_output(&PushOutputMessage {
    session_id: "session-1".into(),
    sequence: 42,
    data_type: "application/octet-stream".into(), // MIME type
    timestamp: 0,
    metadata: None,
    data: vec![0x00, 0x01, 0x02], // rides the raw IPC segment untouched
})?;
```

Since 0.7.0 the SDK prefers the **raw FFI writer** (`PushOutputRawWriterFn`)
when the runner registered one: payload bytes skip JSON serialization and
base64 entirely — only the (usually tiny) metadata is JSON-encoded by the SDK.
This is transparent: `send_push_output` falls back to the legacy JSON writer on
older runners, so extensions compiled with 0.7.x keep working with both
generations, and pre-0.7 extensions run on new runners unchanged.

Runners use `encode_segmented_payload` / `parse_response_payload`
(`[u32 header_len LE][header JSON][binary segment]`) to move binary push
responses to the core without base64; the format discriminator makes it
impossible to confuse with legacy whole-JSON payloads.

## WASM target

The SDK compiles to `wasm32-unknown-unknown` with the same API surface:

```toml
[dependencies]
neomind-extension-sdk = "0.7"

[lib]
crate-type = ["cdylib"]
```

```sh
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown --release
```

Differences on WASM:

- Capability calls are **synchronous** — same functions, selected automatically via `#[cfg]`:
  ```rust
  #[cfg(not(target_arch = "wasm32"))]
  let metrics = device::get_metrics(&context, "device-1").await?;  // async (Native)

  #[cfg(target_arch = "wasm32")]
  let metrics = device::get_metrics(&context, "device-1")?;        // sync (WASM)
  ```
- Platform access goes through host functions (`host_invoke_capability`, `host_event_subscribe`, `host_event_poll`, `host_log`, `host_timestamp_ms`, `host_free`)
- Events use a polling model; memory is managed by the host

## Test kit

Enable the `testkit` feature in `[dev-dependencies]` to test your extension at
the **IPC protocol level** — the same message flow as the real
neomind-extension-runner, over in-memory channels. It catches deadlocks in
command/event handlers, wrong capability call parameters, event routing errors,
and stream session lifecycle leaks that plain unit tests cannot.

```rust ignore
use neomind_extension_sdk::testkit::*;

#[tokio::test]
async fn test_analyze_command() {
    let mut kit = TestKit::new(MyExtension::new());
    kit.start().await;

    let result = kit.execute_command("analyze", json!({"image": "..."})).await
        .expect("command should complete within 5s");
    assert!(result["success"].as_bool().unwrap());

    // Verify capability calls made by the extension
    let calls = kit.capability_calls("device_metrics_write");
    assert_eq!(calls.len(), 3);
}
```

## ABI stability

The IPC boundary types (`neomind_extension_sdk::ipc`) are the stable protocol
between extensions and the main process. Extensions compiled against older SDK
versions keep working because:

1. Messages are serialized as JSON over IPC — only the JSON format matters, not the implementation
2. New fields use `#[serde(default)]` for forward compatibility
3. New optional FFI exports (like the raw push-writer registration) are resolved dynamically; runners that don't know them never look them up

Minimum NeoMind core version: **0.5.0**.

## Safety requirements

Extensions **must** be compiled with `panic = "unwind"` — the runner catches
panics at the FFI boundary and converts them into error responses instead of
aborting the runner process:

```toml
[profile.release]
panic = "unwind"   # required!
opt-level = 3
lto = "thin"
```

## Naming conventions

- Extension IDs are plain kebab-case, e.g. `weather-forecast`, `image-analyzer`, `yolo-video`
- The built library file is `libneomind_extension_{name}.{dylib|so|wasm}`

## License

MIT OR Apache-2.0.
