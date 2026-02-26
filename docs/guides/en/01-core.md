# Core Module

**Package**: `neomind-core`
**Version**: 0.5.8
**Completion**: 90%
**Purpose**: Defines core traits and types for the entire project

## Overview

The Core module is the foundation of the NeoMind project, defining the core abstractions and types that all other modules depend on. It contains no concrete implementations, only interface definitions.

## Module Structure

```
crates/core/src/
├── lib.rs                  # Public interface exports
├── event.rs                # Event type definitions
├── eventbus.rs             # Event bus implementation
├── priority_eventbus.rs    # Priority event bus
├── message.rs              # Message type definitions
├── session.rs              # Session type definitions
├── llm/
│   ├── backend.rs          # LLM runtime trait
│   ├── modality.rs         # Multi-modal content support
│   └── memory_consolidation.rs  # Memory consolidation
├── tools/
│   └── mod.rs              # Tool trait definitions
├── storage/
│   └── mod.rs              # Storage trait definitions
├── integration/
│   ├── mod.rs              # Integration trait
│   ├── connector.rs        # Connector trait
│   └── transformer.rs      # Data transformer trait
├── datasource/
│   ├── mod.rs              # Data source ID system
│   └── types.rs            # DataSourceId types
├── extension/
│   ├── mod.rs              # Extension system
│   ├── types.rs            # Extension types
│   ├── registry.rs         # Extension registry
│   └── loader/             # Extension loaders
├── alerts/
│   └── mod.rs              # Alert system
├── config.rs               # Configuration constants
├── error.rs                # Error types
└── macros.rs               # Macro definitions
```

## Core Traits

### 1. LlmRuntime - LLM Runtime Interface

Defines the interface that all LLM backends must implement.

```rust
#[async_trait]
pub trait LlmRuntime: Send + Sync {
    /// Get backend capabilities
    fn capabilities(&self) -> BackendCapabilities;

    /// Generate text (non-streaming)
    fn generate(&self, input: &LlmInput) -> Result<LlmOutput>;

    /// Generate text (streaming)
    fn generate_stream(&self, input: &LlmInput) -> StreamResult;

    /// Embedding generation (optional)
    fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
}
```

**BackendCapabilities**:
```rust
pub struct BackendCapabilities {
    /// Supports streaming output
    pub streaming: bool,
    /// Supports function calling
    pub function_calling: bool,
    /// Supports vision input
    pub vision: bool,
    /// Supports thinking mode
    pub thinking: bool,
}
```

### 2. Tool - Tool Interface

Defines the interface for AI-callable tools.

```rust
pub trait Tool: Send + Sync {
    /// Tool definition (name, description, parameters)
    fn definition(&self) -> &ToolDefinition;

    /// Execute tool
    fn execute(&self, input: &serde_json::Value) -> Result<ToolOutput>;

    /// Validate input
    fn validate(&self, input: &serde_json::Value) -> Result<()> {
        // Default implementation
    }
}
```

### 3. Integration - Integration Interface

Defines the interface for external system integration.

```rust
#[async_trait]
pub trait Integration: Send + Sync {
    /// Get metadata
    fn metadata(&self) -> &IntegrationMetadata;

    /// Get current state
    fn state(&self) -> IntegrationState;

    /// Start integration
    async fn start(&self) -> Result<()>;

    /// Stop integration
    async fn stop(&self) -> Result<()>;

    /// Subscribe to event stream
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = IntegrationEvent> + Send + '_>>;

    /// Send command
    async fn send_command(&self, command: IntegrationCommand) -> Result<IntegrationResponse>;
}
```

### 4. Extension - Extension Interface

Defines the interface for dynamically loaded extensions. The V2 Extension trait separates metrics from commands:

```rust
#[async_trait]
pub trait Extension: Send + Sync {
    /// Get extension metadata
    fn metadata(&self) -> &ExtensionMetadata;

    /// Declare metrics provided by this extension
    fn metrics(&self) -> &[MetricDescriptor] { &[] }

    /// Declare commands supported by this extension
    fn commands(&self) -> &[ExtensionCommand] { &[] }

    /// Execute a command (async)
    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value>;

    /// Produce metric data (sync for dylib compatibility)
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { Ok(Vec::new()) }

    /// Health check (async, optional)
    async fn health_check(&self) -> Result<bool> { Ok(true) }

    /// Runtime configuration (optional)
    async fn configure(&mut self, config: &Value) -> Result<()> { Ok(()) }
}
```

See [Extension Development Guide](16-extension-dev.md) for complete API details.

### 5. DataSourceId - Data Source Identifier

Provides type-safe data source identification, supporting device, extension, and transform data sources.

```rust
pub struct DataSourceId {
    /// Data source type
    pub source_type: DataSourceType,
    /// Data source ID
    pub source_id: String,
    /// Field path
    pub field_path: String,
}

pub enum DataSourceType {
    Device,
    Extension,
    Transform,
}

impl DataSourceId {
    /// Parse data source ID
    pub fn new(id: &str) -> Result<Self>;

    /// Get device_id part for TimeSeriesStorage
    pub fn device_part(&self) -> String;

    /// Get metric part for TimeSeriesStorage
    pub fn metric_part(&self) -> &str;

    /// Get full storage key
    pub fn storage_key(&self) -> String;
}
```

**Format**: `{source_type}:{source_id}:{field_path}`

| Type | Format | device_part | metric_part |
|------|------|-------------|-------------|
| Device | `{device_id}:{field_path}` | `{device_id}` | `{field_path}` |
| Extension | `extension:{ext_id}:{field_path}` | `extension:{ext_id}` | `{field_path}` |
| Transform | `transform:{trans_id}:{field_path}` | `transform:{trans_id}` | `{field_path}` |

### 6. StorageBackend - Storage Interface

Defines the generic storage interface.

```rust
pub trait StorageBackend: Send + Sync {
    /// Write data
    async fn write(&self, key: &str, value: &[u8]) -> Result<()>;

    /// Read data
    async fn read(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete data
    async fn delete(&self, key: &str) -> Result<()>;

    /// List keys
    async fn list(&self, prefix: &str) -> Result<Vec<String>>;
}
```

## Core Types

### Message - Message Type

```rust
pub struct Message {
    pub role: MessageRole,
    pub content: Content,
    pub timestamp: Option<i64>,
    pub metadata: Option<EventMetadata>,
}

pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

pub enum Content {
    Text(String),
    Parts(Vec<ContentPart>),  // Multi-modal support
}

pub enum ContentPart {
    Text { text: String },
    Image { image: ImageContent },
    Thinking { content: String },  // Reasoning content
}
```

### NeoMindEvent - Event Type

```rust
pub enum NeoMindEvent {
    // Device events
    DeviceOnline { device_id: String, timestamp: i64 },
    DeviceOffline { device_id: String, timestamp: i64 },
    DeviceMetric { device_id: String, metric: String, value: MetricValue },
    DeviceCommandResult { device_id: String, command: String, success: bool },

    // Rule events
    RuleEvaluated { rule_id: String, result: bool },
    RuleTriggered { rule_id: String, trigger_value: serde_json::Value },

    // Workflow events
    WorkflowTriggered { workflow_id: String },
    WorkflowStepCompleted { workflow_id: String, step: String },
    WorkflowCompleted { workflow_id: String },

    // LLM events
    PeriodicReviewTriggered { review_id: String },
    LlmDecisionProposed { decision_id: String, title: String },
    LlmDecisionExecuted { decision_id: String, success: bool },

    // Message events
    MessageCreated { message_id: String, severity: MessageSeverity },
    MessageAcknowledged { message_id: String },
}
```

### Session - Session Type

```rust
pub struct Session {
    pub id: SessionId,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: Vec<Message>,
    pub metadata: SessionMetadata,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);
```

## EventBus - Event Bus

```rust
pub struct EventBus {
    // Channel capacity
    capacity: usize,
    // Broadcast sender
    sender: broadcast::Sender<NeoMindEvent>,
}

impl EventBus {
    /// Create new event bus
    pub fn new() -> Self;

    /// Create event bus with capacity
    pub fn with_capacity(capacity: usize) -> Self;

    /// Publish event
    pub fn publish(&self, event: NeoMindEvent);

    /// Subscribe to all events
    pub fn subscribe(&self) -> EventBusReceiver;

    /// Create filter
    pub fn filter(&self) -> FilterBuilder;
}
```

### FilterBuilder - Event Filtering

```rust
pub struct FilterBuilder<'a> {
    bus: &'a EventBus,
    filters: Vec<FilterFn>,
}

impl<'a> FilterBuilder<'a> {
    /// Only receive device events
    pub fn device_events(self) -> FilteredReceiver;

    /// Only receive rule events
    pub fn rule_events(self) -> FilteredReceiver;

    /// Custom filter
    pub fn custom<F>(self, f: F) -> FilteredReceiver
    where
        F: Fn(&NeoMindEvent) -> bool + Send + 'static;
}
```

## Configuration Constants

```rust
// LLM provider configuration
pub const DEFAULT_OLLAMA_ENDPOINT: &str = "http://localhost:11434";
pub const DEFAULT_OPENAI_ENDPOINT: &str = "https://api.openai.com/v1";

// Default models
pub fn models() -> Vec<&'static str> {
    vec![
        "qwen3-vl:2b",     // Ollama default
        "gpt-4o-mini",     // OpenAI
        "claude-3-5-sonnet", // Anthropic
    ]
}

// API endpoints
pub fn endpoints() -> HashMap<String, String> {
    // ...
}
```

## Usage Examples

### Creating EventBus

```rust
use neomind_core::EventBus;
use neomind_core::NeoMindEvent;

#[tokio::main]
async fn main() {
    let bus = EventBus::new();

    // Subscribe to all events
    let mut rx = bus.subscribe();

    // Subscribe to device events
    let mut device_rx = bus.filter().device_events();

    // Publish event
    bus.publish(NeoMindEvent::DeviceOnline {
        device_id: "sensor_1".to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    });

    // Receive events
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            println!("Received: {:?}", event);
        }
    });
}
```

### Using LlmRuntime Trait

```rust
use neomind_core::llm::backend::{LlmRuntime, LlmInput, GenerationParams};

async fn call_llm(runtime: &dyn LlmRuntime, prompt: &str) -> Result<String> {
    let input = LlmInput {
        messages: vec![
            Message::user(prompt)
        ],
        params: GenerationParams::default(),
        model: None,
    };

    let output = runtime.generate(&input)?;
    Ok(output.text)
}
```

## Error Handling

```rust
pub enum Error {
    /// LLM related errors
    Llm(LlmError),

    /// Storage errors
    Storage(StorageError),

    /// Tool errors
    Tool(ToolError),

    /// Integration errors
    Integration(IntegrationError),

    /// Extension errors
    Extension(ExtensionError),

    /// IO errors
    Io(std::io::Error),

    /// Other errors
    Other(anyhow::Error),
}
```

## Dependencies

```
Core (neomind-core)
    │
    ├── No external dependencies (trait definitions only)
    │
    └── Depends on all other crates
        ├── llm
        ├── agent
        ├── devices
        ├── tools
        ├── storage
        ├── integrations
        └── ...
```

## Design Principles

1. **Minimal Dependencies**: Core module has no dependencies on other business modules
2. **Trait-First**: Define interfaces through traits, allowing different implementations
3. **Type Safety**: Use Rust type system to ensure correctness
4. **Async-First**: All I/O operations are asynchronous
5. **Extensibility**: Loose coupling through traits and events
