# Tools Module

**Package**: `neomind-tools`
**Version**: 0.5.9
**Completion**: 80%
**Purpose**: AI function calling tools

## Overview

The Tools module implements a tool system for AI-callable functions, including device control, rule management, data analysis, and more.

## Module Structure

```
crates/neomind-tools/src/
├── lib.rs                      # Public interface
├── tool.rs                     # Tool trait
├── registry.rs                 # Tool registry
├── core_tools.rs               # Core business tools
├── agent_tools.rs              # Agent tools
├── system_tools.rs             # System tools
├── extension_tools.rs          # Extension tools
├── real.rs                     # Real implementation (feature-gated)
└── simplified.rs               # Simplified interface
```

## Core Trait

### Tool - Tool Interface

```rust
pub trait Tool: Send + Sync {
    /// Get tool definition
    fn definition(&self) -> &ToolDefinition;

    /// Execute tool
    fn execute(&self, input: &serde_json::Value) -> Result<ToolOutput>;

    /// Validate input
    fn validate(&self, input: &serde_json::Value) -> Result<()> {
        // Default implementation: schema-based validation
    }

    /// Get tool schema (for LLM)
    fn schema(&self) -> serde_json::Value {
        // Returns OpenAI function calling format
    }
}
```

### ToolDefinition - Tool Definition

```rust
pub struct ToolDefinition {
    /// Tool name (unique identifier)
    pub name: String,

    /// Display name
    pub display_name: String,

    /// Tool description (for AI)
    pub description: String,

    /// Parameter definitions
    pub parameters: Vec<Parameter>,

    /// Return value description
    pub returns: Option<String>,

    /// Usage examples
    pub examples: Vec<ToolExample>,
}
```

```rust
pub struct Parameter {
    /// Parameter name
    pub name: String,

    /// Parameter type
    pub param_type: ParameterType,

    /// Description
    pub description: String,

    /// Required
    pub required: bool,

    /// Default value
    pub default: Option<serde_json::Value>,

    /// Enum values (for selection)
    pub enum_values: Option<Vec<String>>,
}
```

```rust
pub enum ParameterType {
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
}
```

### ToolOutput - Tool Output

```rust
pub struct ToolOutput {
    /// Success status
    pub success: bool,

    /// Output data
    pub data: serde_json::Value,

    /// Error message (if failed)
    pub error: Option<String>,

    /// Metadata
    pub metadata: HashMap<String, String>,
}
```

## Tool Registry

```rust
pub struct ToolRegistry {
    /// Tool mapping
    tools: HashMap<String, Arc<dyn Tool>>,

    /// Execution history
    history: Arc<RwLock<ToolExecutionHistory>>,

    /// Config
    config: RegistryConfig,
}
```

```rust
impl ToolRegistry {
    /// Create empty registry
    pub fn new() -> Self;

    /// Add tool
    pub fn register(&mut self, tool: Arc<dyn Tool>);

    /// Get tool
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>>;

    /// List all tools
    pub fn list(&self) -> Vec<String>;

    /// Execute tool
    pub async fn execute(
        &self,
        name: &str,
        input: &serde_json::Value,
    ) -> Result<ToolOutput>;
}
```

## Built-in Tools

### Device Tools

```rust
/// List devices
pub struct ListDevicesTool {
    device_service: Arc<DeviceService>,
}

// Input: { "device_type": "sensor" } (optional filter)
// Output: { "devices": [...] }
```

```rust
/// Query device data
pub struct QueryDataTool {
    device_service: Arc<DeviceService>,
    time_series: Arc<TimeSeriesStore>,
}

// Input: { "device_id": "sensor_1", "metric": "temperature" }
// Output: { "current": 25.5, "history": [...] }
```

```rust
/// Control device
pub struct ControlDeviceTool {
    device_service: Arc<DeviceService>,
}

// Input: { "device_id": "relay_1", "command": "turn_on" }
// Output: { "success": true, "result": ... }
```

### Rule Tools

```rust
/// List rules
pub struct ListRulesTool {
    rule_service: Arc<RuleService>,
}
```

```rust
/// Create rule
pub struct CreateRuleTool {
    rule_service: Arc<RuleService>,
    parser: RuleParser,
}
```

### Agent Tools

```rust
/// List agents
pub struct ListAgentsTool {
    agent_service: Arc<AgentService>,
}
```

### System Tools

```rust
/// System info
pub struct SystemInfoTool;

// Output: { "version": "...", "uptime": "...", "memory": ... }
```

## Tool Execution History

```rust
pub struct ToolExecutionHistory {
    /// Total executions
    pub total_executions: usize,

    /// Success count
    pub success_count: usize,

    /// Failure count
    pub failure_count: usize,

    /// Average duration
    pub avg_duration_ms: f64,

    /// Most used tools
    pub most_used_tools: Vec<(String, usize)>,
}
```

## LLM Format

```rust
pub struct LlmToolDefinition {
    /// Tool name
    pub name: String,

    /// Display name
    pub display_name: String,

    /// Description
    pub description: String,

    /// Parameters (JSON Schema format)
    pub parameters: serde_json::Value,
}
```

## Usage Examples

### Create Tool Registry

```rust
use neomind_tools::{ToolRegistryBuilder, QueryDataTool, ControlDeviceTool};
use std::sync::Arc;

let registry = ToolRegistryBuilder::new()
    .with_tool(Arc::new(QueryDataTool::mock()))
    .with_tool(Arc::new(ControlDeviceTool::mock()))
    .with_standard_tools()
    .build();
```

### Execute Tool

```rust
let result = registry.execute(
    "query_data",
    serde_json::json!({
        "device_id": "sensor_1",
        "metric": "temperature"
    }),
).await?;

if result.success {
    println!("Result: {}", result.data);
} else {
    println!("Error: {}", result.error.unwrap());
}
```

## Feature Flags

```toml
[features]
default = ["device", "rule", "agent", "system"]
device = ["mqtt", "http", "webhook"]
rule = ["pest", "executor"]
agent = ["llm"]
system = ["stats", "restart"]
```

## Design Principles

1. **Unified Interface**: All tools implement the same Trait
2. **Type Safety**: Strongly typed input/output
3. **Testable**: Mock implementations provided
4. **LLM-Friendly**: Standardized function calling format
5. **Traceable**: Full execution history tracking
EOF
